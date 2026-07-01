//! SQLite HTTP test for the application hints handlers (`routes/platform.rs`):
//! list, create/replace, and clear — exercised without a Postgres container.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::app::build_router;
use pmp_iq::platform::AnalysisResult;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const ANALYSIS: &str = r#"{"application":{"name":"svc","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
"languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],"platforms":[],
"external":[],"dependencies":[],"components":[],"use_cases":[],"users":[],"groups":[],"access":[]}"#;

async fn seed(db: &pmp_iq::db::Database) -> Uuid {
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
            name: "svc".into(),
            full_name: "org/svc".into(),
            clone_url: "https://x.invalid/svc.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    store::platform_writer(db).write(repo.id, &AnalysisResult::parse(ANALYSIS).unwrap()).await.unwrap()
}

async fn send(app: &Router, cookies: &[String], method: &str, uri: &str, body: Value) -> (u16, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(COOKIE, cookie_header(cookies))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let s = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (s, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
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
async fn application_hints_create_list_and_clear() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let app_id = seed(&db).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let base = format!("/api/platform/applications/{app_id}/hints");

    // Empty initially.
    assert!(get(&app, &cookies, &base).await["hints"].as_array().unwrap().is_empty());

    // A blank entity_type is rejected.
    let (bad, _) = send(&app, &cookies, "PUT", &base, json!({ "entity_type": "  ", "hint": "x" })).await;
    assert_eq!(bad, 400);

    // Create a hint.
    let (ok, _) = send(
        &app,
        &cookies,
        "PUT",
        &base,
        json!({ "entity_type": "infrastructure", "entity_key": "Postgres", "hint": "it is the primary store" }),
    )
    .await;
    assert_eq!(ok, 200);
    assert_eq!(get(&app, &cookies, &base).await["hints"].as_array().unwrap().len(), 1);

    // An empty hint clears it.
    let (cleared, body) = send(
        &app,
        &cookies,
        "PUT",
        &base,
        json!({ "entity_type": "infrastructure", "entity_key": "Postgres", "hint": "" }),
    )
    .await;
    assert_eq!(cleared, 200);
    assert_eq!(body["deleted"], json!(true));
    assert!(get(&app, &cookies, &base).await["hints"].as_array().unwrap().is_empty());
}
