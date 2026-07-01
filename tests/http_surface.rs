//! Broad SQLite HTTP-surface test: exercises the page, platform, jobs and
//! settings route handlers end-to-end without Docker (the existing equivalents
//! use a Postgres container). Asserts every endpoint responds without a server
//! error and key list/detail/facet APIs return 200.

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

const ANALYSIS: &str = r#"{"application":{"name":"web","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
"languages":[{"name":"Rust","percentage":100}],"libraries":[],"infrastructure":[{"name":"Postgres","kind":"database"}],
"tools":[],"cloud_providers":[],"services":[],"platforms":[],"external":[],"dependencies":[],
"components":[{"name":"Api","kind":"controller"}],"use_cases":[],"users":[],"groups":[],"access":[]}"#;

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
            name: "web".into(),
            full_name: "org/web".into(),
            clone_url: "https://x.invalid/web.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    store::platform_writer(db).write(repo.id, &AnalysisResult::parse(ANALYSIS).unwrap()).await.unwrap()
}

async fn status(app: &Router, cookies: &[String], uri: &str) -> u16 {
    app.clone()
        .oneshot(Request::get(uri).header(COOKIE, cookie_header(cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn http_surface_responds_without_server_errors() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let app_id = seed(&db).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Pages render (200) or redirect (3xx) — never a 5xx / auth bounce.
    let pages = [
        "/", "/healthz", "/settings", "/jobs", "/platform", "/platform/graph", "/platform/dashboard",
        "/platform/c4", "/platform/campaigns", "/platform/audit", "/platform/applications",
        "/platform/infrastructure", "/platform/libraries",
    ];
    for uri in pages {
        let s = status(&app, &cookies, uri).await;
        assert!(s < 400, "GET {uri} → {s}");
    }
    assert_eq!(status(&app, &cookies, &format!("/platform/applications/{app_id}")).await, 200);

    // JSON APIs across the route modules return 200.
    let apis = [
        "/api/jobs", "/api/jobs/types", "/api/jobs/executions",
        "/api/platform/applications", "/api/platform/applications/facets",
        "/api/platform/infrastructure", "/api/platform/libraries",
        "/api/platform/dashboard", "/api/platform/c4",
        "/api/settings/entity-kinds", "/api/settings/entity-properties",
        "/api/settings/extraction-prompts",
    ];
    for uri in apis {
        assert_eq!(status(&app, &cookies, uri).await, 200, "GET {uri}");
    }

    // The graph API returns a node/edge structure including the seeded app.
    let resp = app
        .clone()
        .oneshot(Request::get("/api/platform/graph").header(COOKIE, cookie_header(&cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let graph: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(graph["nodes"].as_array().unwrap().iter().any(|n| n["data"]["label"] == "web"));

    // Unauthenticated access is bounced (login redirect or 401), not a 5xx.
    let anon = app
        .clone()
        .oneshot(Request::get("/api/platform/applications").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(anon.status().as_u16() == 302 || anon.status().as_u16() == 401 || anon.status().is_redirection());
}
