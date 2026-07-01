//! Integration test for production-readiness scorecards (M43) on SQLite: the
//! per-application scorecard evaluates default checks against the model +
//! metrics + ownership, and the fleet view ranks applications.

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
use pmp_iq::rbac::TeamInput;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

const ANALYSIS: &str = r#"{"application":{"name":"svc","app_type":"api","description":"a documented service","primary_language":"Rust","metadata":{}},
"languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],"platforms":[],
"external":[],"dependencies":[],
"components":[{"name":"Api","kind":"controller","observability_signals":[{"name":"latency","kind":"metric"}]}],
"use_cases":[{"name":"Pay","components":["Api"],"diagrams":[{"name":"seq","kind":"sequence","content":"sequenceDiagram"}]}],
"endpoints":[],"users":[],"groups":[],"access":[]}"#;

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
async fn scorecard_evaluates_and_fleet_ranks() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let app_id = seed(&db).await;

    // A passing app: good metrics + an owning team.
    store::application_metrics(&db)
        .record(app_id, "llm", &[
            Metric::new("coverage_pct", 80.0, Some("percent")),
            Metric::new("complexity_avg", 5.0, None),
            Metric::new("has_ci", 1.0, Some("bool")),
            Metric::new("tests_total", 10.0, Some("count")),
        ])
        .await
        .unwrap();
    let team = store::teams(&db).create(TeamInput { name: "platform".into(), tenant_id: None }).await.unwrap();
    store::teams(&db).set_owner(team.id, app_id).await.unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // All default checks pass → gold.
    let card = get(&app, &cookies, &format!("/api/platform/applications/{app_id}/scorecard")).await;
    assert_eq!(card["level"], "gold", "{card}");
    assert!((card["score"].as_f64().unwrap() - 1.0).abs() < 1e-9);
    let results = card["results"].as_array().unwrap();
    assert!(results.iter().any(|r| r["check_id"] == "has_owner" && r["passed"] == true));
    assert!(results.iter().any(|r| r["check_id"] == "coverage_min" && r["passed"] == true));

    // The fleet view ranks the application with its level.
    let fleet = get(&app, &cookies, "/api/platform/scorecards").await;
    let rows = fleet["scorecards"].as_array().unwrap();
    let svc = rows.iter().find(|r| r["name"] == "svc").unwrap();
    assert_eq!(svc["level"], "gold");
}

#[tokio::test]
async fn scorecard_flags_unowned_low_coverage_app() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let app_id = seed(&db).await;
    // No team, no metrics → most checks fail → bronze/at_risk.
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let card = get(&app, &cookies, &format!("/api/platform/applications/{app_id}/scorecard")).await;
    let results = card["results"].as_array().unwrap();
    assert!(results.iter().any(|r| r["check_id"] == "has_owner" && r["passed"] == false));
    assert!(results.iter().any(|r| r["check_id"] == "coverage_min" && r["passed"] == false));
    assert!(card["score"].as_f64().unwrap() < 0.85);
}
