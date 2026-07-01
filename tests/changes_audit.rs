//! Integration tests for the platform change feed + audit log (M36) on SQLite:
//! the writer emits precise create/update/remove events across re-syncs, the
//! diff endpoint summarises them, and operator actions are audited.

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
use uuid::Uuid;

fn analysis(desc: &str, deps: &str) -> String {
    format!(
        r#"{{"application":{{"name":"shop","app_type":"api","description":"{desc}","primary_language":"Rust","metadata":{{}}}},
        "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
        "services":[],"platforms":[],"external":[],"dependencies":[{deps}],"components":[],
        "use_cases":[],"users":[],"groups":[],"access":[]}}"#
    )
}

async fn repo_id(db: &pmp_iq::db::Database) -> Uuid {
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
    store::repo_records(db)
        .upsert(RepoRecordInput {
            account_id: account.id,
            name: "shop".into(),
            full_name: "org/shop".into(),
            clone_url: "https://x.invalid/shop.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap()
        .id
}

#[tokio::test]
async fn writer_emits_create_update_remove_changes() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let repo = repo_id(&db).await;
    let writer = store::platform_writer(&db);

    // First sync: app created + two dependencies created.
    let v1 = analysis("d1", r#"{"target_name":"stripe","kind":"http"},{"target_name":"kafka","kind":"queue"}"#);
    let app_id = writer.write(repo, &AnalysisResult::parse(&v1).unwrap()).await.unwrap();

    // Second sync: description changed (app updated), kafka removed, redis added.
    let v2 = analysis("d2", r#"{"target_name":"stripe","kind":"http"},{"target_name":"redis","kind":"cache"}"#);
    writer.write(repo, &AnalysisResult::parse(&v2).unwrap()).await.unwrap();

    let rows = store::platform_changes(&db).timeline(Some(app_id), 100).await.unwrap();
    let has = |etype: &str, key: &str, change: &str| {
        rows.iter().any(|r| r.entity_type == etype && r.entity_key == key && r.change == change)
    };
    assert!(has("application", "shop", "created"), "{rows:?}");
    assert!(has("dependency", "stripe", "created"));
    assert!(has("dependency", "kafka", "created"));
    assert!(has("application", "shop", "updated"), "app updated on desc change");
    assert!(has("dependency", "redis", "created"));
    assert!(has("dependency", "kafka", "removed"));
    // stripe persisted across syncs → only created once, never removed.
    assert!(!has("dependency", "stripe", "removed"));
}

#[tokio::test]
async fn diff_endpoint_summarizes_net_changes() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let repo = repo_id(&db).await;
    let v1 = analysis("d1", r#"{"target_name":"stripe","kind":"http"}"#);
    store::platform_writer(&db).write(repo, &AnalysisResult::parse(&v1).unwrap()).await.unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let resp = app
        .clone()
        .oneshot(Request::get("/api/platform/diff").header(COOKIE, cookie_header(&cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["summary"]["application"]["created"], 1);
    assert_eq!(body["summary"]["dependency"]["created"], 1);
}

#[tokio::test]
async fn app_timeline_and_audit_page_render() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let repo = repo_id(&db).await;
    let v1 = analysis("d1", r#"{"target_name":"stripe","kind":"http"}"#);
    let app_id = store::platform_writer(&db).write(repo, &AnalysisResult::parse(&v1).unwrap()).await.unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Per-application timeline returns the recorded changes.
    let body = get_json(&app, &cookies, &format!("/api/platform/applications/{app_id}/timeline")).await;
    assert!(!body["changes"].as_array().unwrap().is_empty());

    // Global timeline + the admin audit HTML page both render.
    let global = get_json(&app, &cookies, "/api/platform/timeline").await;
    assert!(global["changes"].is_array());
    let page = app
        .clone()
        .oneshot(Request::get("/platform/audit").header(COOKIE, cookie_header(&cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(page.status(), 200);

    // diff with explicit from/to timestamps.
    let diff = get_json(
        &app,
        &cookies,
        "/api/platform/diff?from=2020-01-01T00:00:00Z&to=2999-01-01T00:00:00Z",
    )
    .await;
    assert_eq!(diff["summary"]["application"]["created"], 1);
}

async fn get_json(app: &axum::Router, cookies: &[String], uri: &str) -> Value {
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
async fn login_is_audited_and_visible_to_admin() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let resp = app
        .clone()
        .oneshot(Request::get("/api/audit").header(COOKIE, cookie_header(&cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let events = body["events"].as_array().unwrap();
    assert!(events.iter().any(|e| e["action"] == "login" && e["actor"] == "admin"), "{body}");
}
