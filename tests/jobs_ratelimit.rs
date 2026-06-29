//! End-to-end test (SQLite, no container): the review job self-pauses when the
//! git provider rate-limits it, recording a resume time from the rate-limit
//! headers.

mod common;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platiq::app::build_router;
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;

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

/// A mock GitHub API that always rate-limits the repository listing.
async fn start_rate_limited_github() -> String {
    async fn repos() -> Response {
        ([("retry-after", "2"), ("x-ratelimit-remaining", "0")], StatusCode::TOO_MANY_REQUESTS)
            .into_response()
    }
    let app = Router::new().route("/user/repos", get(repos)).route("/user", get(repos));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{}", addr.port())
}

#[tokio::test]
async fn review_job_self_pauses_on_rate_limit() {
    let base_url = start_rate_limited_github().await;
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // A GitHub account pointed at the rate-limited mock.
    let account = json!({
        "name": "gh", "provider_type": "github", "auth_type": "token",
        "token": "ghp_x", "base_url": base_url, "selection_mode": "all"
    });
    let create = app
        .clone()
        .oneshot(authed(Method::POST, "/api/settings/accounts", &cookies, Some(account)))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);

    // Run the review job.
    let job = json!({ "name": "review", "job_type": "sync-repositories", "trigger_type": "manual" });
    let job_id = body_json(
        app.clone()
            .oneshot(authed(Method::POST, "/api/jobs", &cookies, Some(job)))
            .await
            .unwrap(),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let execution_id = body_json(
        app.clone()
            .oneshot(authed(Method::POST, &format!("/api/jobs/{job_id}/run"), &cookies, None))
            .await
            .unwrap(),
    )
    .await["execution_id"]
        .as_str()
        .unwrap()
        .to_string();

    // The execution pauses (rather than failing) with a resume time.
    let mut exec = Value::Null;
    for _ in 0..60 {
        exec = body_json(
            app.clone()
                .oneshot(authed(
                    Method::GET,
                    &format!("/api/jobs/executions/{execution_id}"),
                    &cookies,
                    None,
                ))
                .await
                .unwrap(),
        )
        .await["execution"]
            .clone();
        let status = exec["status"].as_str().unwrap_or("");
        if status == "paused" || status == "failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(exec["status"], "paused", "logs: {}", exec["logs"]);
    assert!(!exec["resume_at"].is_null(), "resume_at should be set from rate-limit headers");
}
