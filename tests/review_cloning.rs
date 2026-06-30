//! Integration test for the `sync-repositories` cloning stage: a real local
//! git repository is discovered, cloned, and recorded.

mod common;
use pmp_iq::store;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use common::{TestDb, build_state, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::app::build_router;
use serde_json::{Value, json};
use std::path::Path;
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

/// Create a git repo with one commit at `path`.
fn init_repo(path: &Path) {
    let repo = git2::Repository::init(path).unwrap();
    std::fs::write(path.join("README.md"), "# sample\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("README.md")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = git2::Signature::now("Tester", "tester@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
}

#[tokio::test]
async fn clones_local_repository_and_records_it() {
    // Source directory containing one git working copy.
    let base = std::env::temp_dir().join(format!("pi-src-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(base.join("service-a")).unwrap();
    init_repo(&base.join("service-a"));
    let base_str = base.to_string_lossy().to_string();

    let db = TestDb::start().await;
    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Configure a local account.
    let account = json!({
        "name": "local",
        "provider_type": "local",
        "auth_type": "none",
        "base_url": base_str,
        "selection_mode": "all"
    });
    let create_account = app
        .clone()
        .oneshot(authed(Method::POST, "/api/settings/accounts", &cookies, Some(account)))
        .await
        .unwrap();
    assert_eq!(create_account.status(), StatusCode::OK);

    // Create and run the sync-repositories job.
    let job = json!({ "name": "review", "job_type": "sync-repositories", "trigger_type": "manual" });
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
    let mut status = String::new();
    for _ in 0..60 {
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
        if matches!(status.as_str(), "succeeded" | "failed") {
            summary = exec["execution"]["summary"].clone();
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    assert_eq!(status, "succeeded");
    assert_eq!(summary["cloned"], 1);
    assert_eq!(summary["failed"], 0);

    // The repository record should be cloned (path + commit recorded).
    let records = store::repo_records(&db.database())
        .list()
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.full_name, "service-a");
    assert!(record.local_path.is_some());
    assert!(record.last_commit_sha.as_ref().map(|s| s.len() == 40).unwrap_or(false));
    assert!(Path::new(record.local_path.as_ref().unwrap()).join(".git").exists());

    std::fs::remove_dir_all(&base).ok();
}
