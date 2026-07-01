//! Integration test for the codebase-map route (M28): a cloned checkout yields a
//! directory graph; an un-cloned repository is rejected.

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
use std::fs;
use tower::ServiceExt;
use uuid::Uuid;

const ANALYSIS: &str = r#"{
  "application": {"name":"api","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
  "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
  "services":[],"platforms":[],"external":[],"dependencies":[],"components":[],
  "use_cases":[],"users":[],"groups":[],"access":[]
}"#;

/// Seed an application; when `clone_at` is set, mark the repository cloned there.
async fn seed(db: &pmp_iq::db::Database, clone_at: Option<&str>) -> Uuid {
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
    if let Some(path) = clone_at {
        store::repo_records(db).mark_cloned(repo.id, path, "sha").await.unwrap();
    }
    store::platform_writer(db)
        .write(repo.id, &AnalysisResult::parse(ANALYSIS).unwrap())
        .await
        .unwrap()
}

async fn get_map(app: &axum::Router, cookies: &[String], app_id: Uuid) -> (u16, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::get(format!("/api/platform/applications/{app_id}/codebase-map"))
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
async fn codebase_map_of_a_cloned_checkout() {
    // A real temp checkout with a couple of directories.
    let dir = std::env::temp_dir().join(format!("pi-cm-{}", Uuid::new_v4()));
    fs::create_dir_all(dir.join("src/api")).unwrap();
    fs::create_dir_all(dir.join("tests")).unwrap();
    fs::write(dir.join("README.md"), "hi").unwrap();

    let sqlite = SqliteDb::start().await;
    let app_id = seed(&sqlite.database(), Some(&dir.to_string_lossy())).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let (status, map) = get_map(&app, &cookies, app_id).await;
    assert_eq!(status, 200, "map should build: {map}");
    let ids: Vec<&str> =
        map["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"src"), "has src dir: {ids:?}");
    assert!(ids.contains(&"src/api"), "has nested dir: {ids:?}");
    assert!(ids.contains(&"tests"));

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn codebase_map_requires_a_cloned_repository() {
    let sqlite = SqliteDb::start().await;
    let app_id = seed(&sqlite.database(), None).await; // not cloned
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let (status, _) = get_map(&app, &cookies, app_id).await;
    assert_eq!(status, 400);
}
