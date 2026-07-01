//! Integration test for batch-change campaigns (M30): create a fleet-wide
//! campaign, then read its per-repository progress.

mod common;

use axum::body::Body;
use axum::http::Request;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::ai::{AiProfileInput, AiProviderType};
use pmp_iq::app::build_router;
use pmp_iq::platform::AnalysisResult;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
use serde_json::{Value, json};
use tower::ServiceExt;

const ANALYSIS: &str = r#"{
  "application": {"name":"api","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
  "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
  "services":[],"platforms":[],"external":[],"dependencies":[],"components":[],
  "use_cases":[],"users":[],"groups":[],"access":[]
}"#;

async fn seed_app(db: &pmp_iq::db::Database) {
    let account = store::accounts(db)
        .create(AccountInput {
            name: "gh".into(),
            provider_type: ProviderType::Github,
            auth_type: AuthType::Token,
            base_url: None,
            organization: None,
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
    store::platform_writer(db)
        .write(repo.id, &AnalysisResult::parse(ANALYSIS).unwrap())
        .await
        .unwrap();
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
}

#[tokio::test]
async fn create_list_and_track_a_campaign() {
    let sqlite = SqliteDb::start().await;
    seed_app(&sqlite.database()).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // A fleet-wide campaign (no filter → all applications).
    let resp = app
        .clone()
        .oneshot(
            Request::post("/api/platform/campaigns")
                .header(CONTENT_TYPE, "application/json")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::from(r#"{"name":"Bump library","instruction":"bump the dependency"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["execution_ids"].as_array().unwrap().len(), 1);
    let campaign_id = body["campaign"]["id"].as_str().unwrap().to_string();
    assert_eq!(body["campaign"]["name"], "Bump library");

    // It appears in the list.
    let (_, list) = get_json(&app, "/api/platform/campaigns", &cookies).await;
    assert_eq!(list["campaigns"].as_array().unwrap().len(), 1);

    // Its detail exposes one repository target.
    let (status, detail) =
        get_json(&app, &format!("/api/platform/campaigns/{campaign_id}"), &cookies).await;
    assert_eq!(status, 200);
    assert_eq!(detail["targets"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn campaign_requires_name_and_instruction() {
    let sqlite = SqliteDb::start().await;
    seed_app(&sqlite.database()).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let resp = app
        .clone()
        .oneshot(
            Request::post("/api/platform/campaigns")
                .header(CONTENT_TYPE, "application/json")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::from(r#"{"name":"","instruction":""}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
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
