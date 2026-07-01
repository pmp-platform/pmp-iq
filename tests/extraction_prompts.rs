//! Integration tests for configurable extraction prompts (M34): the Settings
//! API to list/save/reset per-section templates, with placeholder validation,
//! and that an override flows into the composed analyzer prompt. SQLite only.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use http_body_util::BodyExt;
use pmp_iq::app::build_router;
use pmp_iq::store;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn get_json(app: &Router, cookies: &[String], uri: &str) -> Value {
    let resp = app
        .clone()
        .oneshot(Request::get(uri).header(COOKIE, cookie_header(cookies)).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "GET {uri}");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn send(app: &Router, cookies: &[String], method: &str, uri: &str, body: Value) -> u16 {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header(COOKIE, cookie_header(cookies))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status().as_u16()
}

#[tokio::test]
async fn prompts_list_save_validate_reset() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;

    // List returns the seeded code defaults for every section.
    let listed = get_json(&app, &cookies, "/api/settings/extraction-prompts").await;
    let sections = listed["sections"].as_array().unwrap();
    assert!(sections.iter().any(|s| s["section"] == "base"));
    assert!(sections.iter().any(|s| s["section"] == "metrics"));
    let base = sections.iter().find(|s| s["section"] == "base").unwrap();
    assert_eq!(base["overridden"], json!(false));
    assert!(base["required_placeholders"].as_array().unwrap().iter().any(|p| p == "{{json_schema}}"));

    // Editing 'members' is accepted and marked overridden.
    assert_eq!(
        send(&app, &cookies, "PUT", "/api/settings/extraction-prompts/members",
            json!({ "template": "Custom members rule.", "enabled": true })).await,
        200
    );
    let after = get_json(&app, &cookies, "/api/settings/extraction-prompts").await;
    let members = after["sections"].as_array().unwrap().iter().find(|s| s["section"] == "members").unwrap();
    assert_eq!(members["overridden"], json!(true));
    assert_eq!(members["template"], json!("Custom members rule."));

    // Editing 'base' without the required schema placeholder is rejected.
    assert_eq!(
        send(&app, &cookies, "PUT", "/api/settings/extraction-prompts/base",
            json!({ "template": "no placeholder", "enabled": true })).await,
        400
    );

    // Reset removes the override.
    assert_eq!(
        send(&app, &cookies, "POST", "/api/settings/extraction-prompts/members/reset", json!({})).await,
        200
    );
    let reset = get_json(&app, &cookies, "/api/settings/extraction-prompts").await;
    let members2 = reset["sections"].as_array().unwrap().iter().find(|s| s["section"] == "members").unwrap();
    assert_eq!(members2["overridden"], json!(false));
}

#[tokio::test]
async fn saved_override_flows_into_loaded_config() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let service = pmp_iq::analysis_config::AnalysisConfigService::new(
        store::entity_kinds(&db),
        store::entity_properties(&db),
        store::extraction_prompts(&db),
    );
    service.save_prompt("dependencies", "Only HTTP dependencies.", true).await.unwrap();

    let cfg = service.load().await.unwrap();
    let prompt = pmp_iq::platform::prompts::compose_system_prompt(&cfg.prompts, &cfg);
    assert!(prompt.contains("Only HTTP dependencies."));
    // The default dependencies prose is replaced.
    assert!(!prompt.contains("self-hosted runtime backing services"));
    // Schema is still always injected.
    assert!(prompt.contains("\"application\":{\"name\":string"));

    // Disabling the metrics section blanks the metrics preamble.
    service.save_prompt("metrics", "x", false).await.unwrap();
    assert_eq!(service.metrics_prompt().await.unwrap(), "");
}
