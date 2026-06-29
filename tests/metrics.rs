//! Integration tests for M31 application metrics: the dual-engine repository on
//! SQLite, and the collect/read routes.

mod common;

use axum::body::Body;
use axum::http::Request;
use axum::http::header::COOKIE;
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platiq::accounts::{AccountInput, AuthType, ProviderType, SelectionMode};
use platiq::ai::{AiProfileInput, AiProviderType};
use platiq::app::build_router;
use platiq::metrics::Metric;
use platiq::platform::AnalysisResult;
use platiq::repositories::RepoRecordInput;
use platiq::store;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const ANALYSIS: &str = r#"{
  "application": {"name":"api","app_type":"api","description":"d","primary_language":"Rust","metadata":{}},
  "languages":[],"libraries":[],"infrastructure":[],"tools":[],"cloud_providers":[],
  "services":[],"platforms":[],"external":[],"dependencies":[],"components":[],
  "use_cases":[],"users":[],"groups":[],"access":[]
}"#;

async fn seed_app(db: &platiq::db::Database) -> Uuid {
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
            name: "api".into(),
            full_name: "org/api".into(),
            clone_url: "https://example.invalid/org/api.git".into(),
            default_branch: Some("main".into()),
        })
        .await
        .unwrap();
    let app_id = store::platform_writer(db)
        .write(repo.id, &AnalysisResult::parse(ANALYSIS).unwrap())
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

#[tokio::test]
async fn metrics_repository_records_and_reads_latest() {
    let sqlite = SqliteDb::start().await;
    let app_id = Uuid::new_v4();
    sqlx::query("INSERT INTO applications (id, name) VALUES (?, ?)")
        .bind(app_id)
        .bind("demo")
        .execute(&sqlite.pool)
        .await
        .unwrap();
    let repo = store::application_metrics(&sqlite.database());

    repo.record(
        app_id,
        "llm",
        &[
            Metric::new("coverage_pct", 83.5, Some("percent")),
            Metric::new("loc", 21450.0, Some("count")),
            Metric::new("has_ci", 1.0, Some("bool")),
        ],
    )
    .await
    .unwrap();

    let latest = repo.latest_for_application(app_id).await.unwrap();
    assert_eq!(latest.len(), 3);
    let cov = latest.iter().find(|m| m.metric_key == "coverage_pct").unwrap();
    assert_eq!(cov.value, 83.5);
    assert_eq!(cov.unit.as_deref(), Some("percent"));

    let all = repo.latest_all().await.unwrap();
    assert!(all.iter().any(|m| m.metric_key == "loc" && m.value == 21450.0));
}

#[tokio::test]
async fn collect_route_enqueues_and_get_returns_metrics() {
    let sqlite = SqliteDb::start().await;
    let app_id = seed_app(&sqlite.database()).await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Collect enqueues an execution.
    let resp = app
        .clone()
        .oneshot(
            Request::post(format!("/api/platform/applications/{app_id}/metrics"))
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body.get("execution_id").is_some());

    // Record metrics directly, then GET returns them.
    store::application_metrics(&sqlite.database())
        .record(app_id, "llm", &[Metric::new("coverage_pct", 90.0, Some("percent"))])
        .await
        .unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::get(format!("/api/platform/applications/{app_id}/metrics"))
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["metrics"].as_array().unwrap().len(), 1);
    assert_eq!(body["metrics"][0]["metric_key"], "coverage_pct");
}

#[tokio::test]
async fn dashboard_api_aggregates_metrics() {
    let sqlite = SqliteDb::start().await;
    let app_id = seed_app(&sqlite.database()).await; // application named "api"
    store::application_metrics(&sqlite.database())
        .record(
            app_id,
            "llm",
            &[Metric::new("coverage_pct", 75.0, Some("percent")), Metric::new("has_ci", 1.0, Some("bool"))],
        )
        .await
        .unwrap();
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let resp = app
        .clone()
        .oneshot(
            Request::get("/api/platform/dashboard")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body["rollup"]["applications"].as_i64().unwrap() >= 1);
    assert_eq!(body["rollup"]["with_ci"], 1);
    assert_eq!(body["leaderboards"]["top_coverage"][0]["name"], "api");
}
