//! End-to-end test: the review-repositories job clones a local repo and
//! analyses it through a (mocked) Anthropic endpoint, populating the platform
//! model.

mod common;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use common::{TestDb, build_state, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platform_inspector::app::build_router;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tower::ServiceExt;

const ANALYSIS_JSON: &str = r#"{"application":{"name":"service-a","app_type":"api","primary_language":"Rust"},
"languages":[{"name":"Rust","percentage":100}],
"libraries":[{"name":"axum","ecosystem":"cargo","version":"0.7","scope":"runtime"}],
"infrastructure":[{"name":"PostgreSQL","kind":"database","version":"16"}],
"dependencies":[],"users":[],"groups":[],"access":[]}"#;

async fn body_json(resp: Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn authed(method: Method, path: &str, cookies: &[String], body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(COOKIE, cookie_header(cookies));
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
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
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
async fn review_job_analyses_and_populates_platform_model() {
    let base_url = start_mock_anthropic().await;

    // A local git repository to analyse.
    let base = std::env::temp_dir().join(format!("pi-src-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(base.join("service-a")).unwrap();
    init_repo(&base.join("service-a"));
    let base_str = base.to_string_lossy().to_string();

    let db = TestDb::start().await;
    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // AI profile pointing at the mock.
    let profile = json!({
        "name": "mock-anthropic",
        "provider_type": "anthropic",
        "api_key": "test-key",
        "config": { "model": "claude-opus-4-8", "base_url": base_url }
    });
    let create_profile = app
        .clone()
        .oneshot(authed(Method::POST, "/api/settings/ai-profiles", &cookies, Some(profile)))
        .await
        .unwrap();
    let profile_id = body_json(create_profile).await["id"].as_str().unwrap().to_string();

    // Local account.
    let account = json!({
        "name": "local", "provider_type": "local", "auth_type": "none",
        "base_url": base_str, "selection_mode": "all"
    });
    app.clone()
        .oneshot(authed(Method::POST, "/api/settings/accounts", &cookies, Some(account)))
        .await
        .unwrap();

    // Review job configured with the AI profile.
    let job = json!({
        "name": "review", "job_type": "review-repositories", "trigger_type": "manual",
        "config": { "ai_profile_id": profile_id }
    });
    let create_job = app
        .clone()
        .oneshot(authed(Method::POST, "/api/jobs", &cookies, Some(job)))
        .await
        .unwrap();
    let job_id = body_json(create_job).await["id"].as_str().unwrap().to_string();

    let run = app
        .clone()
        .oneshot(authed(Method::POST, &format!("/api/jobs/{job_id}/run"), &cookies, None))
        .await
        .unwrap();
    let execution_id = body_json(run).await["execution_id"].as_str().unwrap().to_string();

    // Poll to completion.
    let mut summary = Value::Null;
    for _ in 0..80 {
        let resp = app
            .clone()
            .oneshot(authed(
                Method::GET,
                &format!("/api/jobs/executions/{execution_id}"),
                &cookies,
                None,
            ))
            .await
            .unwrap();
        let exec = body_json(resp).await;
        if matches!(exec["execution"]["status"].as_str(), Some("succeeded") | Some("failed")) {
            assert_eq!(exec["execution"]["status"], "succeeded", "logs: {}", exec["execution"]["logs"]);
            summary = exec["execution"]["summary"].clone();
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    assert_eq!(summary["cloned"], 1);
    assert_eq!(summary["analyzed"], 1);

    // The platform model is populated.
    let (apps,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM applications")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(apps, 1);
    let (name,): (String,) = sqlx::query_as("SELECT name FROM applications LIMIT 1")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(name, "service-a");
    let (infra,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM infrastructure")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(infra, 1);

    std::fs::remove_dir_all(&base).ok();
}
