//! Integration test for the repository-accounts HTTP API.

mod common;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use common::{TestDb, build_state, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::app::build_router;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn body_json(resp: Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn body_text(resp: Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
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
async fn create_list_and_delete_account() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let payload = json!({
        "name": "gh-main",
        "provider_type": "github",
        "auth_type": "token",
        "token": "ghp_supersecret",
        "selection_mode": "all"
    });
    let create = app
        .clone()
        .oneshot(authed(Method::POST, "/api/settings/accounts", &cookies, Some(payload)))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let created = body_json(create).await;
    assert_eq!(created["name"], "gh-main");
    assert_eq!(created["has_credentials"], true);
    // The plaintext token must never be returned.
    assert!(!created.to_string().contains("ghp_supersecret"));
    let id = created["id"].as_str().unwrap().to_string();

    let list = app
        .clone()
        .oneshot(authed(Method::GET, "/api/settings/accounts", &cookies, None))
        .await
        .unwrap();
    let listed = body_json(list).await;
    assert_eq!(listed["accounts"].as_array().unwrap().len(), 1);
    assert!(!listed.to_string().contains("ghp_supersecret"));

    let del = app
        .clone()
        .oneshot(authed(
            Method::DELETE,
            &format!("/api/settings/accounts/{id}"),
            &cookies,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::OK);
}

#[tokio::test]
async fn local_account_preview_lists_git_repos() {
    // A temp directory containing one git working copy and one plain folder.
    let base = std::env::temp_dir().join(format!("pi-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(base.join("service-a/.git")).unwrap();
    std::fs::create_dir_all(base.join("not-a-repo")).unwrap();
    let base_str = base.to_string_lossy().to_string();

    let db = TestDb::start().await;
    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let payload = json!({
        "name": "local",
        "provider_type": "local",
        "auth_type": "none",
        "base_url": base_str,
        "selection_mode": "all"
    });
    let create = app
        .clone()
        .oneshot(authed(Method::POST, "/api/settings/accounts", &cookies, Some(payload)))
        .await
        .unwrap();
    let id = body_json(create).await["id"].as_str().unwrap().to_string();

    let preview = app
        .clone()
        .oneshot(authed(
            Method::GET,
            &format!("/api/settings/accounts/{id}/repositories"),
            &cookies,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(preview.status(), StatusCode::OK);
    let repos = body_json(preview).await;
    let arr = repos["repositories"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "service-a");

    std::fs::remove_dir_all(&base).ok();
}

#[tokio::test]
async fn api_requires_authentication() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));

    let resp = app
        .oneshot(Request::get("/api/settings/accounts").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let _ = body_text(resp).await;
}
