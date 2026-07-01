//! SQLite HTTP test for the analysis-config handlers (`routes/analysis_config.rs`
//! and `analysis_config::service`): entity-kind and entity-property CRUD —
//! without a Postgres container.

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
    let s = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (s, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
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
async fn entity_kind_crud() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let before = get(&app, &cookies, "/api/settings/entity-kinds").await["kinds"].as_array().unwrap().len();
    let (cs, created) = send(
        &app,
        &cookies,
        "POST",
        "/api/settings/entity-kinds",
        json!({ "entity_type": "services", "kind_id": "zzz_custom", "name": "Custom", "description": "x" }),
    )
    .await;
    assert_eq!(cs, 200, "{created}");
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(get(&app, &cookies, "/api/settings/entity-kinds").await["kinds"].as_array().unwrap().len(), before + 1);

    let (us, updated) = send(
        &app,
        &cookies,
        "PUT",
        &format!("/api/settings/entity-kinds/{id}"),
        json!({ "entity_type": "services", "kind_id": "zzz_custom", "name": "Custom Renamed", "description": "x" }),
    )
    .await;
    assert_eq!(us, 200);
    assert_eq!(updated["name"], "Custom Renamed");

    let (ds, _) = send(&app, &cookies, "DELETE", &format!("/api/settings/entity-kinds/{id}"), json!({})).await;
    assert_eq!(ds, 200);
}

#[tokio::test]
async fn entity_property_crud() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let (cs, created) = send(
        &app,
        &cookies,
        "POST",
        "/api/settings/entity-properties",
        json!({ "entity_type": "applications", "prop_id": "zzz_prop", "name": "Framework", "description": "", "data_type": "string" }),
    )
    .await;
    assert_eq!(cs, 200, "{created}");
    let id = created["id"].as_str().unwrap().to_string();
    assert!(get(&app, &cookies, "/api/settings/entity-properties").await["properties"].as_array().unwrap().iter().any(|p| p["prop_id"] == "zzz_prop"));

    let (us, updated) = send(
        &app,
        &cookies,
        "PUT",
        &format!("/api/settings/entity-properties/{id}"),
        json!({ "entity_type": "applications", "prop_id": "zzz_prop", "name": "Web framework", "description": "", "data_type": "string" }),
    )
    .await;
    assert_eq!(us, 200);
    assert_eq!(updated["name"], "Web framework");

    let (ds, _) = send(&app, &cookies, "DELETE", &format!("/api/settings/entity-properties/{id}"), json!({})).await;
    assert_eq!(ds, 200);
}
