//! Integration test for version currency & tech radar (M45) on SQLite: an app's
//! libraries are assessed against the policy (lag + EOL), and the radar is
//! operator-curated.

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
use pmp_iq::techradar::PolicyInput;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const ANALYSIS: &str = r#"{"application":{"name":"svc","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
"languages":[],"libraries":[{"name":"old","ecosystem":"cargo","version":"1.0.0"},{"name":"axum","ecosystem":"cargo","version":"0.8.0"}],
"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],"platforms":[],"external":[],"dependencies":[],
"components":[],"use_cases":[],"endpoints":[],"users":[],"groups":[],"access":[]}"#;

async fn seed(db: &pmp_iq::db::Database) -> Uuid {
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
async fn currency_assesses_libraries_against_policy() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let app_id = seed(&db).await;
    // `old` is 2 majors behind and end-of-life; `axum` is current.
    let policies = store::techradar(&db);
    policies.upsert_policy(PolicyInput { ecosystem: "cargo".into(), name: "old".into(), latest: Some("3.0.0".into()), eol_date: Some("2020-01-01".into()) }).await.unwrap();
    policies.upsert_policy(PolicyInput { ecosystem: "cargo".into(), name: "axum".into(), latest: Some("0.8.0".into()), eol_date: None }).await.unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    let body = get(&app, &cookies, &format!("/api/platform/applications/{app_id}/currency")).await;
    let deps = body["dependencies"].as_array().unwrap();
    let old = deps.iter().find(|d| d["name"] == "old").unwrap();
    assert_eq!(old["major_behind"], 2);
    assert_eq!(old["eol_status"], "eol");
    let axum = deps.iter().find(|d| d["name"] == "axum").unwrap();
    assert_eq!(axum["major_behind"], 0);
    // 1 of 2 current → score 0.5.
    assert!((body["score"].as_f64().unwrap() - 0.5).abs() < 1e-9);

    // Fleet currency ranks the app (least-current first).
    let fleet = get(&app, &cookies, "/api/platform/currency").await;
    assert!(fleet["currency"].as_array().unwrap().iter().any(|r| r["name"] == "svc"));
}

#[tokio::test]
async fn tech_radar_crud() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let resp = app
        .clone()
        .oneshot(
            Request::post("/api/platform/tech-radar")
                .header(COOKIE, cookie_header(&cookies))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "quadrant": "language", "name": "Rust", "ring": "adopt" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let radar = get(&app, &cookies, "/api/platform/tech-radar").await;
    let entries = radar["radar"].as_array().unwrap();
    assert!(entries.iter().any(|e| e["name"] == "Rust" && e["ring"] == "adopt"));
}
