//! Integration test for API contracts (M42) on SQLite: endpoints are extracted,
//! a consumer's dependency resolves to a producer endpoint, and the endpoints
//! API reports endpoints with their consumers (impact). Disallowed protocols are
//! dropped.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::http::header::COOKIE;
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::app::build_router;
use pmp_iq::platform::AnalysisResult;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

const PRODUCER: &str = r#"{"application":{"name":"billing","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
"languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],"platforms":[],
"external":[],"dependencies":[],
"components":[{"name":"PayController","kind":"controller"}],
"endpoints":[
  {"operation":"POST /charge","protocol":"http","summary":"charge a card","component":"PayController","files":["src/pay.rs"]},
  {"operation":"GET /health","protocol":"http"},
  {"operation":"weird","protocol":"soap"}],
"use_cases":[],"users":[],"groups":[],"access":[]}"#;

const CONSUMER: &str = r#"{"application":{"name":"web","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
"languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],"platforms":[],
"external":[],
"components":[{"name":"Api","kind":"controller"}],
"dependencies":[{"target_name":"billing","kind":"http","component":"Api","endpoint":"POST /charge"}],
"endpoints":[],"use_cases":[],"users":[],"groups":[],"access":[]}"#;

async fn seed(db: &pmp_iq::db::Database, name: &str, analysis: &str) -> Uuid {
    let account = store::accounts(db)
        .create(AccountInput {
            name: format!("a-{name}"),
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
            name: name.into(),
            full_name: format!("org/{name}"),
            clone_url: format!("https://x.invalid/{name}.git"),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    store::platform_writer(db).write(repo.id, &AnalysisResult::parse(analysis).unwrap()).await.unwrap()
}

async fn get(app: &Router, cookies: &[String], uri: &str) -> Value {
    let resp = app
        .clone()
        .oneshot(Request::get(uri).header(COOKIE, cookie_header(cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "GET {uri}");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn endpoints_extracted_and_consumers_resolved() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    // Producer first so the consumer's dependency can resolve to its endpoint.
    let billing = seed(&db, "billing", PRODUCER).await;
    seed(&db, "web", CONSUMER).await;

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let body = get(&app, &cookies, &format!("/api/platform/applications/{billing}/endpoints")).await;
    let endpoints = body["endpoints"].as_array().unwrap();

    // The "soap" endpoint was dropped (unsupported protocol); two http remain.
    assert_eq!(endpoints.len(), 2, "{body}");
    let charge = endpoints
        .iter()
        .find(|e| e["endpoint"]["operation"] == "POST /charge")
        .expect("charge endpoint present");
    assert_eq!(charge["endpoint"]["summary"], "charge a card");
    assert_eq!(charge["endpoint"]["files"][0], "src/pay.rs");

    // The consumer "web" appears as a consumer of POST /charge (impact).
    let consumers = charge["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 1);
    assert_eq!(consumers[0]["name"], "web");

    // GET /health has no consumers.
    let health = endpoints.iter().find(|e| e["endpoint"]["operation"] == "GET /health").unwrap();
    assert!(health["consumers"].as_array().unwrap().is_empty());
}
