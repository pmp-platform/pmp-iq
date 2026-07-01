//! SQLite HTTP test for the jobs route module (`routes/jobs.rs`): list, types,
//! run, executions and pause/resume — exercised without a Postgres container.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::Request;
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::app::build_router;
use pmp_iq::jobs::model::{JobInput, TriggerType};
use pmp_iq::store;
use serde_json::{Value, json};
use tower::ServiceExt;

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

async fn post_status(app: &Router, cookies: &[String], uri: &str) -> u16 {
    app.clone()
        .oneshot(
            Request::post(uri)
                .header(COOKIE, cookie_header(cookies))
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn jobs_api_run_and_inspect_lifecycle() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let job = store::jobs(&db)
        .create(JobInput {
            job_type: "noop".into(),
            name: "smoke".into(),
            trigger_type: TriggerType::Manual,
            cron_expr: None,
            config: json!({}),
            enabled: true,
            next_run_at: None,
        })
        .await
        .unwrap();

    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Registered types include the built-in noop job.
    let types = get(&app, &cookies, "/api/jobs/types").await;
    assert!(types.to_string().contains("noop"));

    // The job is listed.
    let jobs = get(&app, &cookies, "/api/jobs").await;
    assert!(jobs.to_string().contains(&job.id.to_string()));

    // Run it → an execution is created.
    let run = app
        .clone()
        .oneshot(
            Request::post(format!("/api/jobs/{}/run", job.id))
                .header(COOKIE, cookie_header(&cookies))
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(run.status(), 200);
    let bytes = run.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let exec_id = body["execution_id"].as_str().unwrap().to_string();

    // Executions list + detail are reachable.
    let execs = get(&app, &cookies, "/api/jobs/executions").await;
    assert!(execs.to_string().contains(&exec_id));
    assert_eq!(
        status_of(&app, &cookies, &format!("/api/jobs/executions/{exec_id}")).await,
        200
    );

    // Pause/resume handlers respond without a server error (the noop run may
    // already be terminal → a 4xx is acceptable, a 5xx is not).
    assert!(post_status(&app, &cookies, &format!("/api/jobs/executions/{exec_id}/pause")).await < 500);
    assert!(post_status(&app, &cookies, &format!("/api/jobs/executions/{exec_id}/resume")).await < 500);
}

async fn status_of(app: &Router, cookies: &[String], uri: &str) -> u16 {
    app.clone()
        .oneshot(Request::get(uri).header(COOKIE, cookie_header(cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
        .as_u16()
}

async fn send(app: &Router, cookies: &[String], method: &str, uri: &str, body: Value) -> (u16, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(COOKIE, cookie_header(cookies))
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let s = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (s, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn jobs_api_crud_and_execution_page() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Create a job via the API.
    let (cs, created) = send(
        &app,
        &cookies,
        "POST",
        "/api/jobs",
        json!({ "job_type": "noop", "name": "via-api", "trigger_type": "manual" }),
    )
    .await;
    assert_eq!(cs, 200, "{created}");
    let id = created["id"].as_str().unwrap().to_string();

    // Update it (rename).
    let (us, updated) = send(
        &app,
        &cookies,
        "PUT",
        &format!("/api/jobs/{id}"),
        json!({ "job_type": "noop", "name": "renamed", "trigger_type": "manual" }),
    )
    .await;
    assert_eq!(us, 200);
    assert_eq!(updated["name"], "renamed");

    // Run it → an execution exists; its detail HTML page renders.
    let (rs, run) = send(&app, &cookies, "POST", &format!("/api/jobs/{id}/run"), json!({})).await;
    assert_eq!(rs, 200);
    let exec = run["execution_id"].as_str().unwrap();
    assert_eq!(status_of(&app, &cookies, &format!("/jobs/executions/{exec}")).await, 200);

    // Delete the job.
    let (ds, _) = send(&app, &cookies, "DELETE", &format!("/api/jobs/{id}"), json!({})).await;
    assert_eq!(ds, 200);
}
