//! Integration test for application-detail action routes that the other suites
//! don't exercise: `sync`, ask history, and the ask result endpoint.

mod common;

use axum::body::Body;
use axum::http::Request;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platform_inspector::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platform_inspector::ai::{AiProfileInput, AiProviderType};
use platform_inspector::app::build_router;
use platform_inspector::platform::AnalysisResult;
use platform_inspector::repositories::RepoRecordInput;
use platform_inspector::store;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const ANALYSIS: &str = r#"{
  "application": {"name":"api","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
  "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
  "services":[],"platforms":[],"external":[],"dependencies":[],"components":[],
  "use_cases":[],"users":[],"groups":[],"access":[]
}"#;

async fn seed(db: &platform_inspector::db::Database) -> Uuid {
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

async fn get_json(app: &axum::Router, url: &str, cookies: &[String]) -> (u16, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::get(url)
                .header(COOKIE, cookie_header(cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn sync_ask_history_and_ask_result() {
    let sqlite = SqliteDb::start().await;
    let app_id = seed(&sqlite.database()).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Ask history is initially empty.
    let (status, body) = get_json(
        &app,
        &format!("/api/platform/applications/{app_id}/ask"),
        &cookies,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["questions"].as_array().unwrap().len(), 0);

    // Ask a question → get an execution id, then poll its result.
    let resp = app
        .clone()
        .oneshot(
            Request::post(format!("/api/platform/applications/{app_id}/ask"))
                .header(CONTENT_TYPE, "application/json")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::from(r#"{"question":"what is this?"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let execution_id = body["execution_id"].as_str().unwrap();

    let (status, result) = get_json(
        &app,
        &format!("/api/platform/applications/{app_id}/ask/{execution_id}"),
        &cookies,
    )
    .await;
    assert_eq!(status, 200);
    assert!(result.get("status").is_some(), "ask result carries a status: {result}");
}

#[tokio::test]
async fn sync_application_enqueues() {
    let sqlite = SqliteDb::start().await;
    let app_id = seed(&sqlite.database()).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let resp = app
        .clone()
        .oneshot(
            Request::post(format!("/api/platform/applications/{app_id}/sync"))
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, 200, "sync should enqueue: {body}");
    assert!(body.get("execution_id").is_some());
}
