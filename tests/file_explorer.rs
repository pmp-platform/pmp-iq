//! Integration test for M17 on SQLite: use-case/component file attribution is
//! persisted and surfaced in the application detail, and the File Explorer
//! routes browse the cloned checkout while blocking path traversal.

mod common;

use axum::body::Body;
use axum::http::Request;
use axum::http::header::COOKIE;
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platiq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platiq::app::build_router;
use platiq::platform::AnalysisResult;
use platiq::repositories::RepoRecordInput;
use platiq::store;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

const ANALYSIS: &str = r#"{
  "application": {"name":"api","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
  "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
  "services":[],"platforms":[],"external":[],"dependencies":[],
  "components":[{"name":"Svc","kind":"service","description":"d","files":["src/svc.rs"]}],
  "use_cases":[{"name":"Checkout","description":"buy","components":["Svc"],"files":["src/checkout.rs"],"diagrams":[]}],
  "users":[],"groups":[],"access":[]
}"#;

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
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

#[tokio::test]
async fn file_attribution_and_explorer() {
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

    // File attribution is persisted and surfaced in the detail.
    let detail = store::platform_query(&db).detail("applications", app_id).await.unwrap();
    assert_eq!(detail["components"][0]["files"][0], "src/svc.rs");
    assert_eq!(detail["use_cases"][0]["files"][0], "src/checkout.rs");

    // Create a real checkout and point the repository record at it.
    let dir = std::env::temp_dir().join(format!("pi-explorer-{}", Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src").join("main.rs"), "fn main() {}\n").unwrap();
    let local = dir.to_string_lossy().to_string();
    store::repo_records(&db).mark_cloned(repo.id, &local, "abc123").await.unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Browse the root and a subdirectory.
    let (status, body) = get_json(&app, &format!("/api/platform/applications/{app_id}/files"), &cookies).await;
    assert_eq!(status, 200, "root listing: {body}");
    let names: Vec<&str> = body["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"src"), "root has src: {body}");

    let (status, body) = get_json(&app, &format!("/api/platform/applications/{app_id}/files?path=src"), &cookies).await;
    assert_eq!(status, 200);
    assert_eq!(body["entries"][0]["name"], "main.rs");

    // Read a file.
    let (status, body) = get_json(
        &app,
        &format!("/api/platform/applications/{app_id}/files/content?path=src/main.rs"),
        &cookies,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["content"], "fn main() {}\n");

    // Path traversal is rejected.
    let (status, _) = get_json(
        &app,
        &format!("/api/platform/applications/{app_id}/files/content?path=../../../etc/hosts"),
        &cookies,
    )
    .await;
    assert_eq!(status, 400, "traversal blocked");

    let _ = std::fs::remove_dir_all(&dir);
}
