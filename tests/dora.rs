//! Integration test for DORA metrics (M47) on SQLite: deployment + incident
//! events arrive via the generic event API, the per-application and fleet DORA
//! reports compute over them, and the summary is recorded as trending metrics.

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
"external":[],"dependencies":[],"components":[],"use_cases":[],"endpoints":[],"users":[],"groups":[],"access":[]}"#;

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

async fn send(app: &Router, req: Request<Body>) -> (u16, Value) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn get(uri: &str, cookies: &[String]) -> Request<Body> {
    Request::get(uri).header(COOKIE, cookie_header(cookies)).body(Body::empty()).unwrap()
}

fn post(uri: &str, cookies: &[String], body: Value) -> Request<Body> {
    Request::post(uri)
        .header(COOKIE, cookie_header(cookies))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn captures_events_and_computes_dora() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let app_id = seed(&db).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Two successful deploys (one resolvable by repository name) + one failed.
    for succeeded in [true, true, false] {
        let (status, _) = send(
            &app,
            post(
                "/api/events/deploy",
                &cookies,
                json!({ "application_id": app_id.to_string(), "succeeded": succeeded }),
            ),
        )
        .await;
        assert_eq!(status, 200);
    }
    // Repository-name resolution path.
    let (status, body) = send(
        &app,
        post("/api/events/deploy", &cookies, json!({ "repository_full_name": "org/svc" })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let deploy_id = body["id"].as_str().unwrap().to_string();

    // An incident attributed to that deploy, then resolved.
    let (status, body) =
        send(&app, post("/api/events/incident", &cookies, json!({ "application_id": app_id.to_string(), "caused_by": deploy_id }))).await;
    assert_eq!(status, 200);
    let incident_id = body["id"].as_str().unwrap();
    let (status, _) = send(&app, post(&format!("/api/events/incident/{incident_id}/resolve"), &cookies, json!({}))).await;
    assert_eq!(status, 200);

    // Per-application DORA reflects the four events.
    let (status, body) = send(&app, get(&format!("/api/platform/applications/{app_id}/dora"), &cookies)).await;
    assert_eq!(status, 200, "{body}");
    let s = &body["summary"];
    assert_eq!(s["deployments"], 4);
    assert_eq!(s["incidents"], 1);
    // 1 incident / 4 deploys = 0.25 change-failure rate.
    assert!((s["change_failure_rate"].as_f64().unwrap() - 0.25).abs() < 1e-9, "{s}");
    assert!(s["tier"].is_string());

    // The summary was recorded as trending metrics (M31 category = delivery).
    let metrics = store::application_metrics(&db).latest_for_application(app_id).await.unwrap();
    let cfr = metrics.iter().find(|m| m.metric_key == "dora_change_failure_rate").unwrap();
    assert_eq!(cfr.category, "delivery");
    assert!((cfr.value - 0.25).abs() < 1e-9);

    // The fleet report includes the application (it has delivery events).
    let (status, body) = send(&app, get("/api/platform/dora", &cookies)).await;
    assert_eq!(status, 200);
    let rows = body["applications"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], app_id.to_string());
    assert_eq!(body["fleet"]["deployments"], 4);
}

#[tokio::test]
async fn empty_fleet_dora_is_safe() {
    let sqlite = SqliteDb::start().await;
    let _ = seed(&sqlite.database()).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let (status, body) = send(&app, get("/api/platform/dora", &cookies)).await;
    assert_eq!(status, 200);
    assert_eq!(body["applications"].as_array().unwrap().len(), 0);
    assert_eq!(body["fleet"]["deployments"], 0);
    assert_eq!(body["fleet"]["tier"], "low");
}
