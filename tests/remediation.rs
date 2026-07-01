//! Integration test for auto-remediation (M46) on SQLite: a rule whose trigger
//! matches an application's signals proposes a deduplicated remediation, and
//! approving it opens an agent task (flipping the remediation to `running`).

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use pmp_iq::ai::{AiProfileInput, AiProviderType};
use pmp_iq::app::build_router;
use pmp_iq::metrics::Metric;
use pmp_iq::platform::AnalysisResult;
use pmp_iq::repositories::RepoRecordInput;
use pmp_iq::store;
use pmp_iq::techradar::repository::PolicyInput;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const ANALYSIS: &str = r#"{"application":{"name":"svc","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
"languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],"platforms":[],
"external":[],"dependencies":[],"components":[],"use_cases":[],"endpoints":[],"users":[],"groups":[],"access":[]}"#;

const ANALYSIS_LIB: &str = r#"{"application":{"name":"svc","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
"languages":[],"libraries":[{"name":"oldlib","ecosystem":"cargo","version":"1.0.0"}],
"infrastructure":[],"tools":[],"cloud_providers":[],"services":[],"platforms":[],
"external":[],"dependencies":[],"components":[],"use_cases":[],"endpoints":[],"users":[],"groups":[],"access":[]}"#;

async fn seed(db: &pmp_iq::db::Database, coverage: f64) -> Uuid {
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
    let app_id = store::platform_writer(db).write(repo.id, &AnalysisResult::parse(ANALYSIS).unwrap()).await.unwrap();
    store::application_metrics(db)
        .record(app_id, "llm", &[Metric::new("coverage_pct", coverage, Some("percent"))])
        .await
        .unwrap();
    store::ai_profiles(db)
        .create(AiProfileInput {
            name: "cli".into(),
            provider_type: AiProviderType::ClaudeCli,
            config: json!({ "binary_path": "true" }),
            secrets_enc: None,
            enabled: true,
        })
        .await
        .unwrap();
    app_id
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
async fn rule_proposes_and_approval_opens_agent_task() {
    let sqlite = SqliteDb::start().await;
    let app_id = seed(&sqlite.database(), 30.0).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // A coverage-below-70 rule.
    let (status, _) = send(
        &app,
        post(
            "/api/platform/remediation/rules",
            &cookies,
            json!({
                "name": "low-coverage",
                "trigger_kind": "metric_below",
                "params": { "metric": "coverage_pct", "threshold": 70 },
                "action": "agent_task",
                "prompt": "Raise test coverage above 70%."
            }),
        ),
    )
    .await;
    assert_eq!(status, 200);

    // Evaluate the fleet → the low-coverage app is proposed.
    let (status, body) = send(&app, post("/api/platform/remediation/evaluate", &cookies, json!({}))).await;
    assert_eq!(status, 200);
    assert_eq!(body["proposed"], 1, "{body}");

    // The proposal appears in the queue keyed by the application id.
    let (_, body) = send(&app, get("/api/platform/remediations?status=proposed", &cookies)).await;
    let rows = body["remediations"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let rem_id = rows[0]["id"].as_str().unwrap().to_string();
    assert_eq!(rows[0]["finding_key"], app_id.to_string());

    // Re-evaluating is deduplicated (no second proposal).
    let (_, body) = send(&app, post("/api/platform/remediation/evaluate", &cookies, json!({}))).await;
    assert_eq!(body["proposed"], 0);

    // Approve → an agent task is opened and the remediation leaves the queue.
    let (status, body) = send(&app, post(&format!("/api/platform/remediations/{rem_id}/approve"), &cookies, json!({}))).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["task_id"].as_str().is_some());

    let (_, body) = send(&app, get("/api/platform/remediations?status=proposed", &cookies)).await;
    assert_eq!(body["remediations"].as_array().unwrap().len(), 0);

    let (_, body) = send(&app, get("/api/platform/remediations?status=running", &cookies)).await;
    assert_eq!(body["remediations"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn healthy_app_proposes_nothing_and_dismiss_clears() {
    let sqlite = SqliteDb::start().await;
    let _app_id = seed(&sqlite.database(), 95.0).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    send(
        &app,
        post(
            "/api/platform/remediation/rules",
            &cookies,
            json!({
                "name": "low-coverage",
                "trigger_kind": "metric_below",
                "params": { "metric": "coverage_pct", "threshold": 70 },
                "action": "agent_task",
                "prompt": "Raise coverage."
            }),
        ),
    )
    .await;

    let (_, body) = send(&app, post("/api/platform/remediation/evaluate", &cookies, json!({}))).await;
    assert_eq!(body["proposed"], 0, "healthy app should not be proposed");

    let (_, body) = send(&app, get("/api/platform/remediations", &cookies)).await;
    assert_eq!(body["remediations"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn page_rule_lifecycle_and_validation() {
    let sqlite = SqliteDb::start().await;
    let _ = seed(&sqlite.database(), 30.0).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // The Remediation page renders.
    let (status, _) = send(&app, get("/platform/remediation", &cookies)).await;
    assert_eq!(status, 200);

    // Create then delete a rule.
    let (_, rule) = send(
        &app,
        post(
            "/api/platform/remediation/rules",
            &cookies,
            json!({ "name": "r", "trigger_kind": "dep_eol", "params": {}, "action": "agent_task", "prompt": "p" }),
        ),
    )
    .await;
    let rule_id = rule["id"].as_str().unwrap();
    let del = Request::delete(format!("/api/platform/remediation/rules/{rule_id}"))
        .header(COOKIE, cookie_header(&cookies))
        .body(Body::empty())
        .unwrap();
    let (status, _) = send(&app, del).await;
    assert_eq!(status, 200);
    let (_, body) = send(&app, get("/api/platform/remediation/rules", &cookies)).await;
    assert_eq!(body["rules"].as_array().unwrap().len(), 0);

    // A rule with an empty prompt is rejected.
    let (status, _) = send(
        &app,
        post(
            "/api/platform/remediation/rules",
            &cookies,
            json!({ "name": "r", "trigger_kind": "dep_eol", "params": {}, "action": "agent_task", "prompt": "" }),
        ),
    )
    .await;
    assert_eq!(status, 400);

    // Approving a non-existent remediation is a 404.
    let (status, _) =
        send(&app, post(&format!("/api/platform/remediations/{}/approve", Uuid::new_v4()), &cookies, json!({}))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn eol_dependency_rule_proposes_and_dismiss_clears() {
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
            name: "svc".into(),
            full_name: "org/svc".into(),
            clone_url: "https://x.invalid/svc.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    let app_id = store::platform_writer(&db).write(repo.id, &AnalysisResult::parse(ANALYSIS_LIB).unwrap()).await.unwrap();
    // A policy marking the library end-of-life in the past.
    store::techradar(&db)
        .upsert_policy(PolicyInput {
            ecosystem: "cargo".into(),
            name: "oldlib".into(),
            latest: Some("2.0.0".into()),
            eol_date: Some("2020-01-01".into()),
        })
        .await
        .unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    send(&app, post(
        "/api/platform/remediation/rules",
        &cookies,
        json!({ "name": "eol", "trigger_kind": "dep_eol", "params": {}, "action": "agent_task", "prompt": "Upgrade EOL deps." }),
    )).await;

    let (_, body) = send(&app, post("/api/platform/remediation/evaluate", &cookies, json!({}))).await;
    assert_eq!(body["proposed"], 1, "EOL dependency should trigger a remediation: {body}");

    let (_, body) = send(&app, get("/api/platform/remediations?status=proposed", &cookies)).await;
    let rem_id = body["remediations"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(body["remediations"][0]["finding_key"], app_id.to_string());

    // Dismiss it → it leaves the proposed queue, lands in dismissed.
    let (status, _) = send(&app, post(&format!("/api/platform/remediations/{rem_id}/dismiss"), &cookies, json!({}))).await;
    assert_eq!(status, 200);
    let (_, body) = send(&app, get("/api/platform/remediations?status=proposed", &cookies)).await;
    assert_eq!(body["remediations"].as_array().unwrap().len(), 0);
    let (_, body) = send(&app, get("/api/platform/remediations?status=dismissed", &cookies)).await;
    assert_eq!(body["remediations"].as_array().unwrap().len(), 1);
}
