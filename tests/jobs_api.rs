//! Integration test for the jobs subsystem: create, run, and observe a job
//! execution end to end against a real database.

mod common;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use common::{TestDb, build_state, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platform_inspector::app::build_router;
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

#[tokio::test]
async fn create_run_and_complete_noop_job() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Create a noop job.
    let payload = json!({ "name": "smoke", "job_type": "noop", "trigger_type": "manual" });
    let create = app
        .clone()
        .oneshot(authed(Method::POST, "/api/jobs", &cookies, Some(payload)))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let job_id = body_json(create).await["id"].as_str().unwrap().to_string();

    // Run it.
    let run = app
        .clone()
        .oneshot(authed(Method::POST, &format!("/api/jobs/{job_id}/run"), &cookies, None))
        .await
        .unwrap();
    assert_eq!(run.status(), StatusCode::OK);
    let execution_id = body_json(run).await["execution_id"].as_str().unwrap().to_string();

    // Poll the execution until it reaches a terminal state.
    let mut status = String::new();
    let mut logs = String::new();
    for _ in 0..40 {
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
        status = exec["execution"]["status"].as_str().unwrap().to_string();
        logs = exec["execution"]["logs"].as_str().unwrap_or("").to_string();
        if matches!(status.as_str(), "succeeded" | "failed") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(status, "succeeded");
    assert!(logs.contains("noop job executed"), "logs were: {logs}");
}

#[tokio::test]
async fn running_unknown_job_type_records_failure() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let payload = json!({ "name": "bad", "job_type": "does-not-exist", "trigger_type": "manual" });
    let create = app
        .clone()
        .oneshot(authed(Method::POST, "/api/jobs", &cookies, Some(payload)))
        .await
        .unwrap();
    let job_id = body_json(create).await["id"].as_str().unwrap().to_string();

    let run = app
        .clone()
        .oneshot(authed(Method::POST, &format!("/api/jobs/{job_id}/run"), &cookies, None))
        .await
        .unwrap();
    let execution_id = body_json(run).await["execution_id"].as_str().unwrap().to_string();

    let mut status = String::new();
    for _ in 0..40 {
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
        status = body_json(resp).await["execution"]["status"].as_str().unwrap().to_string();
        if matches!(status.as_str(), "succeeded" | "failed") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(status, "failed");
}
