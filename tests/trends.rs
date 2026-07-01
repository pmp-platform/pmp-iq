//! Integration tests for metric trends & charts (M35) on SQLite: the series,
//! distribution, per-application series and portfolio read endpoints.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::http::header::COOKIE;
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::app::build_router;
use pmp_iq::metrics::Metric;
use pmp_iq::platform::AnalysisResult;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

async fn seed_app(db: &pmp_iq::db::Database, name: &str) -> Uuid {
    let account = store::accounts(db)
        .create(AccountInput {
            name: format!("a-{name}"),
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
            name: name.into(),
            full_name: format!("org/{name}"),
            clone_url: format!("https://x.invalid/{name}.git"),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    let analysis = format!(
        r#"{{"application":{{"name":"{name}","app_type":"api","description":"d","primary_language":"Rust","metadata":{{}}}},
        "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],
        "platforms":[],"external":[],"dependencies":[],"components":[],"use_cases":[],"users":[],"groups":[],"access":[]}}"#
    );
    store::platform_writer(db).write(repo.id, &AnalysisResult::parse(&analysis).unwrap()).await.unwrap()
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
async fn series_distribution_and_portfolio() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let a = seed_app(&db, "alpha").await;
    let b = seed_app(&db, "beta").await;
    let metrics = store::application_metrics(&db);
    metrics
        .record(a, "llm", &[
            Metric::new("coverage_pct", 80.0, Some("percent")),
            Metric::new("complexity_avg", 5.0, None),
            Metric::new("loc", 1000.0, Some("count")),
        ])
        .await
        .unwrap();
    metrics
        .record(b, "llm", &[Metric::new("coverage_pct", 40.0, Some("percent")), Metric::new("loc", 2000.0, Some("count"))])
        .await
        .unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Platform-wide coverage series has at least one daily point.
    let s = get(&app, &cookies, "/api/platform/series?metric=coverage_pct").await;
    assert!(!s["series"].as_array().unwrap().is_empty());

    // Distribution over the two apps' latest coverage values.
    let d = get(&app, &cookies, "/api/platform/distribution?metric=coverage_pct&buckets=4").await;
    assert_eq!(d["count"], 2);
    assert!(!d["buckets"].as_array().unwrap().is_empty());

    // Disallowed dimension is rejected.
    let bad = app
        .clone()
        .oneshot(
            Request::get("/api/platform/series?metric=coverage_pct&dimension=evil")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);

    // Per-application series + latest.
    let app_s = get(&app, &cookies, &format!("/api/platform/applications/{a}/series?metric=coverage_pct")).await;
    assert_eq!(app_s["latest"], 80.0);

    // Portfolio includes alpha with its coverage + LOC.
    let p = get(&app, &cookies, "/api/platform/portfolio").await;
    let apps = p["apps"].as_array().unwrap();
    let alpha = apps.iter().find(|x| x["id"] == a.to_string()).unwrap();
    assert_eq!(alpha["coverage_pct"], 80.0);
    assert_eq!(alpha["loc"], 1000.0);

    // Dimension-grouped series (by app_type) returns a per-key series map.
    let grouped = get(&app, &cookies, "/api/platform/series?metric=coverage_pct&dimension=app_type").await;
    assert_eq!(grouped["dimension"], "app_type");
    // Both apps are app_type "api" → a single "api" group.
    assert!(grouped["series"]["api"].is_array());
}
