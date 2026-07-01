//! Integration test for the C4 export route (M29 + M38): the projected DSL +
//! Mermaid include the platform's applications (Context), and the Container and
//! Component levels project a single application's datastores and components.

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

const ANALYSIS: &str = r#"{
  "application": {"name":"checkout","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
  "languages":[],"libraries":[],
  "infrastructure":[{"name":"Postgres","kind":"database"}],
  "tools":[],"cloud_providers":[],"services":[],"platforms":[],"external":[],
  "dependencies":[{"target_name":"stripe","kind":"http","component":"PayClient"}],
  "components":[{"name":"Api","kind":"controller"},{"name":"PayClient","kind":"client"}],
  "use_cases":[{"name":"Pay","components":["Api","PayClient"],"diagrams":[]}],
  "users":[],"groups":[],"access":[]
}"#;

/// Seed a single analysed application and return the live router + auth cookies.
async fn setup() -> (SqliteDb, Router, Vec<String>) {
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
            name: "checkout".into(),
            full_name: "org/checkout".into(),
            clone_url: "https://example.invalid/org/checkout.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    store::platform_writer(&db)
        .write(repo.id, &AnalysisResult::parse(ANALYSIS).unwrap())
        .await
        .unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    (sqlite, app, cookies)
}

/// GET a JSON endpoint with auth and return the decoded body.
async fn get_json(app: &Router, cookies: &[String], uri: &str) -> Value {
    let resp = app
        .clone()
        .oneshot(
            Request::get(uri)
                .header(COOKIE, cookie_header(cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "GET {uri}");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn first_application_id(app: &Router, cookies: &[String]) -> String {
    let body = get_json(app, cookies, "/api/platform/applications").await;
    body["items"][0]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn c4_export_includes_applications() {
    let (_db, app, cookies) = setup().await;
    let body = get_json(&app, &cookies, "/api/platform/c4").await;
    let dsl = body["dsl"].as_str().unwrap();
    let mermaid = body["mermaid"].as_str().unwrap();
    assert!(dsl.contains("workspace"));
    assert!(dsl.contains("softwareSystem \"checkout\""), "DSL: {dsl}");
    assert!(mermaid.starts_with("C4Context"));
    assert!(mermaid.contains("\"checkout\""), "mermaid: {mermaid}");
}

#[tokio::test]
async fn c4_container_level_projects_app_datastores() {
    let (_db, app, cookies) = setup().await;
    let id = first_application_id(&app, &cookies).await;
    let uri = format!("/api/platform/c4?level=container&application={id}&dependencies=true");
    let body = get_json(&app, &cookies, &uri).await;
    let mermaid = body["mermaid"].as_str().unwrap();
    assert!(mermaid.starts_with("C4Container"), "mermaid: {mermaid}");
    assert!(mermaid.contains("ContainerDb"), "expected datastore: {mermaid}");
    assert!(mermaid.contains("\"Postgres\""), "mermaid: {mermaid}");
    assert!(mermaid.contains("System_Ext"), "expected external: {mermaid}");
}

#[tokio::test]
async fn c4_component_level_projects_components() {
    let (_db, app, cookies) = setup().await;
    let id = first_application_id(&app, &cookies).await;
    let uri = format!("/api/platform/c4?level=component&application={id}");
    let body = get_json(&app, &cookies, &uri).await;
    let mermaid = body["mermaid"].as_str().unwrap();
    let dsl = body["dsl"].as_str().unwrap();
    assert!(mermaid.starts_with("C4Component"), "mermaid: {mermaid}");
    assert!(mermaid.contains("\"Api\""), "mermaid: {mermaid}");
    assert!(mermaid.contains("\"PayClient\""), "mermaid: {mermaid}");
    // The PayClient → stripe dependency edge is projected.
    assert!(mermaid.contains("System_Ext"), "mermaid: {mermaid}");
    assert!(dsl.contains("component "), "DSL: {dsl}");
}

#[tokio::test]
async fn c4_container_level_requires_application() {
    let (_db, app, cookies) = setup().await;
    let resp = app
        .clone()
        .oneshot(
            Request::get("/api/platform/c4?level=container")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}
