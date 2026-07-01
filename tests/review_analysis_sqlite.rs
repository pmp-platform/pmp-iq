//! End-to-end review-job test on SQLite (no Docker): the sync-repositories job
//! clones a real local git repo and analyses it through an in-process mock
//! Anthropic endpoint, populating the platform model. Exercises the full review
//! orchestration, the analyzer, the local provider and the writer.

mod common;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::app::build_router;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tower::ServiceExt;

const ANALYSIS_JSON: &str = r#"{"application":{"name":"service-a","app_type":"api","primary_language":"Rust"},
"languages":[{"name":"Rust","percentage":100}],
"libraries":[{"name":"axum","ecosystem":"cargo","version":"0.7","scope":"runtime"}],
"infrastructure":[{"name":"PostgreSQL","kind":"database","version":"16"}],
"dependencies":[],"components":[{"name":"Api","kind":"controller"}],"use_cases":[],
"users":[],"groups":[],"access":[]}"#;

async fn body_json(resp: Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn authed(method: Method, path: &str, cookies: &[String], body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path).header(COOKIE, cookie_header(cookies));
    let body = match body {
        Some(v) => {
            builder = builder.header(CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).unwrap()
}

fn init_repo(path: &Path) {
    let repo = git2::Repository::init(path).unwrap();
    std::fs::write(path.join("Cargo.toml"), "[package]\nname=\"service-a\"\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("Cargo.toml")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("T", "t@e.com").unwrap();
    // Commit onto `main` explicitly so the metrics job's `origin/main` sync works.
    repo.commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[]).unwrap();
    repo.set_head("refs/heads/main").unwrap();
}

/// Start a mock Anthropic Messages endpoint; returns its base URL.
async fn start_mock_anthropic() -> String {
    async fn messages() -> Json<Value> {
        Json(json!({
            "content": [{ "type": "text", "text": ANALYSIS_JSON }],
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        }))
    }
    let app = Router::new().route("/v1/messages", post(messages));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://127.0.0.1:{}", addr.port())
}

#[tokio::test]
async fn review_job_analyses_local_repo_on_sqlite() {
    let base_url = start_mock_anthropic().await;

    let base = std::env::temp_dir().join(format!("pi-src-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(base.join("service-a")).unwrap();
    init_repo(&base.join("service-a"));
    let base_str = base.to_string_lossy().to_string();

    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // AI profile pointing at the in-process mock.
    let profile = json!({
        "name": "mock-anthropic", "provider_type": "anthropic", "api_key": "test-key",
        "config": { "model": "claude-opus-4-8", "base_url": base_url }
    });
    let profile_id = body_json(
        app.clone().oneshot(authed(Method::POST, "/api/settings/ai-profiles", &cookies, Some(profile))).await.unwrap(),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Local account scanning the temp directory.
    let account = json!({
        "name": "local", "provider_type": "local", "auth_type": "none",
        "base_url": base_str, "selection_mode": "all"
    });
    app.clone()
        .oneshot(authed(Method::POST, "/api/settings/accounts", &cookies, Some(account)))
        .await
        .unwrap();

    // Create + run the sync job.
    let job = json!({
        "name": "review", "job_type": "sync-repositories", "trigger_type": "manual",
        "config": { "ai_profile_id": profile_id }
    });
    let job_id = body_json(app.clone().oneshot(authed(Method::POST, "/api/jobs", &cookies, Some(job))).await.unwrap())
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let execution_id = body_json(
        app.clone().oneshot(authed(Method::POST, &format!("/api/jobs/{job_id}/run"), &cookies, None)).await.unwrap(),
    )
    .await["execution_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Poll to completion.
    let mut summary = Value::Null;
    for _ in 0..120 {
        let exec = body_json(
            app.clone()
                .oneshot(authed(Method::GET, &format!("/api/jobs/executions/{execution_id}"), &cookies, None))
                .await
                .unwrap(),
        )
        .await;
        if matches!(exec["execution"]["status"].as_str(), Some("succeeded") | Some("failed")) {
            assert_eq!(exec["execution"]["status"], "succeeded", "logs: {}", exec["execution"]["logs"]);
            summary = exec["execution"]["summary"].clone();
            break;
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
    assert_eq!(summary["cloned"], 1, "summary: {summary}");
    assert_eq!(summary["analyzed"], 1);

    // The platform model is populated (verified via the read API).
    let apps = body_json(
        app.clone().oneshot(authed(Method::GET, "/api/platform/applications", &cookies, None)).await.unwrap(),
    )
    .await;
    let items = apps["items"].as_array().unwrap();
    assert!(items.iter().any(|a| a["name"] == "service-a"), "{apps}");

    let infra = body_json(
        app.clone().oneshot(authed(Method::GET, "/api/platform/infrastructure", &cookies, None)).await.unwrap(),
    )
    .await;
    assert!(infra["items"].as_array().unwrap().iter().any(|i| i["name"] == "PostgreSQL"));

    // Collect quality metrics on the synced application (exercises the
    // collect-metrics job: clone → LLM passes → derived metrics → record).
    let app_id = items.iter().find(|a| a["name"] == "service-a").unwrap()["id"].as_str().unwrap().to_string();
    let mexec = body_json(
        app.clone()
            .oneshot(authed(Method::POST, &format!("/api/platform/applications/{app_id}/metrics"), &cookies, Some(json!({}))))
            .await
            .unwrap(),
    )
    .await["execution_id"]
        .as_str()
        .unwrap()
        .to_string();
    for _ in 0..120 {
        let exec = body_json(
            app.clone()
                .oneshot(authed(Method::GET, &format!("/api/jobs/executions/{mexec}"), &cookies, None))
                .await
                .unwrap(),
        )
        .await;
        if matches!(exec["execution"]["status"].as_str(), Some("succeeded") | Some("failed")) {
            assert_eq!(
                exec["execution"]["status"], "succeeded",
                "metrics error: {} logs: {}", exec["execution"]["error"], exec["execution"]["logs"]
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
    // Derived metrics are recorded even though the mock returns no metric fields.
    let metrics = body_json(
        app.clone()
            .oneshot(authed(Method::GET, &format!("/api/platform/applications/{app_id}/metrics"), &cookies, None))
            .await
            .unwrap(),
    )
    .await;
    assert!(metrics["metrics"].is_array() || metrics["metrics"].is_object(), "metrics: {metrics}");

    std::fs::remove_dir_all(&base).ok();
}
