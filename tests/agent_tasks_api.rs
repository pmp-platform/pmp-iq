//! Integration test for the application "AI Agent" routes on SQLite: creating a
//! task, listing tasks, reading the transcript, and posting a follow-up message.

mod common;

use axum::body::Body;
use axum::http::Request;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platiq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platiq::agent_tasks::NewAgentTask;
use platiq::ai::{AiProfileInput, AiProviderType};
use platiq::app::build_router;
use platiq::platform::AnalysisResult;
use platiq::repositories::RepoRecordInput;
use platiq::store;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const ANALYSIS: &str = r#"{
  "application": {"name":"api","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
  "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
  "services":[],"platforms":[],"external":[],"dependencies":[],"components":[],
  "use_cases":[],"users":[],"groups":[],"access":[]
}"#;

/// Seed an application (account + repo + analysis) and an enabled AI profile.
async fn seed_app(db: &platiq::db::Database) -> Uuid {
    let account = store::accounts(db)
        .create(AccountInput {
            name: "gh".into(),
            provider_type: ProviderType::Github,
            auth_type: AuthType::Token,
            base_url: None,
            credentials_enc: None,
            selection_mode: SelectionMode::All,
            selection_value: None,
            enabled: true,
        })
        .await
        .unwrap();
    let repo = store::repo_records(db)
        .upsert(RepoRecordInput {
            account_id: account.id,
            name: "api".into(),
            full_name: "org/api".into(),
            clone_url: "https://example.invalid/org/api.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    let result = AnalysisResult::parse(ANALYSIS).unwrap();
    let app_id = store::platform_writer(db).write(repo.id, &result).await.unwrap();
    store::ai_profiles(db)
        .create(AiProfileInput {
            name: "cli".into(),
            provider_type: AiProviderType::ClaudeCli,
            config: json!({ "binary_path": "true" }),
            secrets_enc: None,
            enabled: true,
        })
        .await
        .unwrap();
    app_id
}

async fn json_body(resp: axum::response::Response) -> (axum::http::StatusCode, Value) {
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

#[tokio::test]
async fn create_list_and_read_an_agent_task() {
    let sqlite = SqliteDb::start().await;
    let app_id = seed_app(&sqlite.database()).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let base = format!("/api/platform/applications/{app_id}/agent-tasks");

    // Create a task (this enqueues the singleton agent-task job once).
    let (status, body) = json_body(
        app.clone()
            .oneshot(
                Request::post(&base)
                    .header(CONTENT_TYPE, "application/json")
                    .header(COOKIE, cookie_header(&cookies))
                    .body(Body::from(r#"{"title":"Add /health","message":"add a health endpoint"}"#))
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 200, "create should succeed: {body}");
    assert!(body.get("execution_id").is_some());
    let task_id = body["task"]["id"].as_str().unwrap().to_string();
    assert_eq!(body["task"]["title"], "Add /health");
    assert!(body["task"]["branch_name"].as_str().unwrap().starts_with("agent/"));

    // List shows the task.
    let (status, body) = json_body(
        app.clone()
            .oneshot(
                Request::get(&base)
                    .header(COOKIE, cookie_header(&cookies))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["tasks"].as_array().unwrap().len(), 1);

    // Read the transcript: the first user message was recorded by the route.
    let detail = format!("{base}/{task_id}");
    let (status, body) = json_body(
        app.clone()
            .oneshot(
                Request::get(&detail)
                    .header(COOKIE, cookie_header(&cookies))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 200);
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "add a health endpoint");
}

#[tokio::test]
async fn post_message_enqueues_a_turn() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let app_id = seed_app(&db).await;
    // Create the task directly (no execution started yet) so the message route's
    // enqueue is not blocked by the per-job "already running" guard.
    let task = store::agent_tasks(&db)
        .create(NewAgentTask {
            application_id: app_id,
            repository_id: Uuid::new_v4(),
            title: "Existing task".into(),
        })
        .await
        .unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let (status, body) = json_body(
        app.clone()
            .oneshot(
                Request::post(format!(
                    "/api/platform/applications/{app_id}/agent-tasks/{}/messages",
                    task.id
                ))
                .header(CONTENT_TYPE, "application/json")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::from(r#"{"message":"also add tests"}"#))
                .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 200, "follow-up should succeed: {body}");
    assert!(body.get("execution_id").is_some());

    // The follow-up message is now on the transcript.
    let (_, body) = json_body(
        app.clone()
            .oneshot(
                Request::get(format!(
                    "/api/platform/applications/{app_id}/agent-tasks/{}",
                    task.id
                ))
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["content"], "also add tests");
}

#[tokio::test]
async fn create_rejects_empty_fields() {
    let sqlite = SqliteDb::start().await;
    let app_id = seed_app(&sqlite.database()).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let base = format!("/api/platform/applications/{app_id}/agent-tasks");

    let (status, _) = json_body(
        app.clone()
            .oneshot(
                Request::post(&base)
                    .header(CONTENT_TYPE, "application/json")
                    .header(COOKIE, cookie_header(&cookies))
                    .body(Body::from(r#"{"title":"","message":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn create_for_unknown_application_is_rejected() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    // An application id that does not resolve to any repository → BadRequest.
    let (status, _) = json_body(
        app.clone()
            .oneshot(
                Request::post(format!("/api/platform/applications/{}/agent-tasks", Uuid::new_v4()))
                    .header(CONTENT_TYPE, "application/json")
                    .header(COOKIE, cookie_header(&cookies))
                    .body(Body::from(r#"{"title":"t","message":"m"}"#))
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 400);
}
