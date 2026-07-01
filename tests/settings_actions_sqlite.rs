//! SQLite HTTP test for the settings account handlers (`routes/settings.rs`):
//! list/create/update/delete — exercised without a Postgres container.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::app::build_router;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn send(app: &Router, cookies: &[String], method: &str, uri: &str, body: Value) -> (u16, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(COOKIE, cookie_header(cookies))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

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

#[tokio::test]
async fn account_crud_lifecycle() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Create.
    let (status, created) = send(
        &app,
        &cookies,
        "POST",
        "/api/settings/accounts",
        json!({
            "name": "gh-main", "provider_type": "github", "auth_type": "token",
            "token": "secret", "selection_mode": "all", "enabled": true
        }),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    let id = created["id"].as_str().unwrap().to_string();

    // List shows it.
    assert_eq!(get(&app, &cookies, "/api/settings/accounts").await["accounts"].as_array().unwrap().len(), 1);

    // Update (disable it).
    let (ustatus, updated) = send(
        &app,
        &cookies,
        "PUT",
        &format!("/api/settings/accounts/{id}"),
        json!({
            "name": "gh-main", "provider_type": "github", "auth_type": "token",
            "selection_mode": "all", "enabled": false
        }),
    )
    .await;
    assert_eq!(ustatus, 200);
    assert_eq!(updated["enabled"], json!(false));

    // Delete.
    let (dstatus, _) = send(&app, &cookies, "DELETE", &format!("/api/settings/accounts/{id}"), json!({})).await;
    assert_eq!(dstatus, 200);
    assert!(get(&app, &cookies, "/api/settings/accounts").await["accounts"].as_array().unwrap().is_empty());
}
