//! Integration test for the application Q&A ("ask the LLM") flow on SQLite:
//! resolving an application to its repository and enqueuing an
//! `llm-repository-request` execution with the question.

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

const ANALYSIS: &str = r#"{
  "application": {"name":"api","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
  "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
  "services":[],"platforms":[],"external":[],"dependencies":[],"components":[],
  "use_cases":[],"users":[],"groups":[],"access":[]
}"#;

#[tokio::test]
async fn ask_resolves_repository_and_enqueues() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();

    let account = store::accounts(&db)
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
    let repo = store::repo_records(&db)
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
    let app_id = store::platform_writer(&db).write(repo.id, &result).await.unwrap();

    // The new query resolves an application back to its repository.
    let resolved = store::platform_query(&db).application_repository(app_id).await.unwrap();
    assert_eq!(resolved, Some(repo.id));

    // Seed an enabled AI profile so the ask route can pick one.
    store::ai_profiles(&db)
        .create(AiProfileInput {
            name: "cli".into(),
            provider_type: AiProviderType::ClaudeCli,
            config: json!({ "binary_path": "true" }),
            secrets_enc: None,
            enabled: true,
        })
        .await
        .unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let resp = app
        .clone()
        .oneshot(
            Request::post(format!("/api/platform/applications/{app_id}/ask"))
                .header(CONTENT_TYPE, "application/json")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::from(r#"{"question":"what does this do?"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status, 200, "ask should succeed: {body}");
    assert!(body.get("execution_id").is_some(), "response carries an execution_id: {body}");
}
