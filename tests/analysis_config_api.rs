//! Integration test for the analysis-config settings API (entity kinds +
//! extraction properties): seeded defaults plus full CRUD.

mod common;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use common::{TestDb, build_state, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platiq::app::build_router;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn body_json(resp: Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn req(method: Method, path: &str, cookies: &[String], body: Option<Value>) -> Request<Body> {
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
async fn entity_kinds_and_properties_crud() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // Migration 009 seeds both tables.
    let kinds = body_json(app.clone().oneshot(req(Method::GET, "/api/settings/entity-kinds", &cookies, None)).await.unwrap()).await;
    assert!(!kinds["kinds"].as_array().unwrap().is_empty(), "kinds seeded");
    let props = body_json(app.clone().oneshot(req(Method::GET, "/api/settings/entity-properties", &cookies, None)).await.unwrap()).await;
    assert!(!props["properties"].as_array().unwrap().is_empty(), "properties seeded");

    // Create a kind (id + name + description), see it listed, update, delete.
    let created = body_json(
        app.clone()
            .oneshot(req(Method::POST, "/api/settings/entity-kinds", &cookies,
                Some(json!({"entity_type": "diagrams", "kind_id": "custom-test", "name": "Custom Test", "description": "a test kind", "config": {"theme": "dark"}}))))
            .await
            .unwrap(),
    )
    .await;
    let kind_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["name"], "Custom Test");
    assert_eq!(created["description"], "a test kind");
    assert_eq!(created["config"]["theme"], "dark");
    let listed = body_json(app.clone().oneshot(req(Method::GET, "/api/settings/entity-kinds", &cookies, None)).await.unwrap()).await;
    assert!(listed["kinds"].as_array().unwrap().iter().any(|k| k["kind_id"] == "custom-test"));

    let updated_kind = body_json(
        app.clone()
            .oneshot(req(Method::PUT, &format!("/api/settings/entity-kinds/{kind_id}"), &cookies,
                Some(json!({"entity_type": "services", "kind_id": "custom-test", "name": "Renamed", "description": "updated"}))))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(updated_kind["name"], "Renamed");
    assert_eq!(updated_kind["description"], "updated");

    let del = app.clone().oneshot(req(Method::DELETE, &format!("/api/settings/entity-kinds/{kind_id}"), &cookies, None)).await.unwrap();
    assert_eq!(del.status(), StatusCode::OK);
    let after = body_json(app.clone().oneshot(req(Method::GET, "/api/settings/entity-kinds", &cookies, None)).await.unwrap()).await;
    assert!(!after["kinds"].as_array().unwrap().iter().any(|k| k["kind_id"] == "custom-test"));

    // Create, update, and delete a property (now with a description).
    let prop = body_json(
        app.clone()
            .oneshot(req(Method::POST, "/api/settings/entity-properties", &cookies,
                Some(json!({"entity_type": "applications", "prop_id": "uptime", "name": "Uptime", "description": "SLA", "data_type": "number"}))))
            .await
            .unwrap(),
    )
    .await;
    let prop_id = prop["id"].as_str().unwrap().to_string();
    assert_eq!(prop["data_type"], "number");
    assert_eq!(prop["description"], "SLA");

    let updated = body_json(
        app.clone()
            .oneshot(req(Method::PUT, &format!("/api/settings/entity-properties/{prop_id}"), &cookies,
                Some(json!({"entity_type": "applications", "prop_id": "uptime", "name": "Uptime %", "description": "uptime ratio", "data_type": "string"}))))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(updated["name"], "Uptime %");
    assert_eq!(updated["description"], "uptime ratio");
    assert_eq!(updated["data_type"], "string");

    let del = app.clone().oneshot(req(Method::DELETE, &format!("/api/settings/entity-properties/{prop_id}"), &cookies, None)).await.unwrap();
    assert_eq!(del.status(), StatusCode::OK);

    // Endpoints require authentication.
    let unauth = app.oneshot(Request::get("/api/settings/entity-kinds").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
}
