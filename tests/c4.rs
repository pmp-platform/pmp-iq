//! Integration test for the C4 export route (M29): the projected DSL + Mermaid
//! include the platform's applications.

mod common;

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
  "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
  "services":[],"platforms":[],"external":[],"dependencies":[],"components":[],
  "use_cases":[],"users":[],"groups":[],"access":[]
}"#;

#[tokio::test]
async fn c4_export_includes_applications() {
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
    let resp = app
        .clone()
        .oneshot(
            Request::get("/api/platform/c4")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let dsl = body["dsl"].as_str().unwrap();
    let mermaid = body["mermaid"].as_str().unwrap();
    assert!(dsl.contains("workspace"));
    assert!(dsl.contains("softwareSystem \"checkout\""), "DSL: {dsl}");
    assert!(mermaid.starts_with("C4Context"));
    assert!(mermaid.contains("\"checkout\""), "mermaid: {mermaid}");
}
