//! Integration test for the AI agent profiles HTTP API and data layer.

mod common;
use platform_inspector::store;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use common::{TestDb, build_state, cookie_header, login_cookies};
use http_body_util::BodyExt;
use platform_inspector::ai::{AiProfileInput, AiProviderType};
use platform_inspector::app::build_router;
use serde_json::{Value, json};
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
async fn repository_crud_round_trip() {
    let db = TestDb::start().await;
    let repo = store::ai_profiles(&db.database());

    let created = repo
        .create(AiProfileInput {
            name: "anthropic".into(),
            provider_type: AiProviderType::Anthropic,
            config: json!({ "model": "claude-opus-4-8" }),
            secrets_enc: Some(vec![9, 9]),
            enabled: true,
        })
        .await
        .unwrap();
    assert_eq!(created.provider_type, AiProviderType::Anthropic);
    assert_eq!(created.config["model"], "claude-opus-4-8");

    let fetched = repo.get(created.id).await.unwrap();
    assert_eq!(fetched.secrets_enc, Some(vec![9, 9]));

    repo.delete(created.id).await.unwrap();
    assert!(repo.get(created.id).await.is_err());
}

#[tokio::test]
async fn create_and_list_via_api_hides_secrets() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let payload = json!({
        "name": "anthropic-main",
        "provider_type": "anthropic",
        "api_key": "sk-very-secret",
        "config": { "model": "claude-opus-4-8", "effort": "high" }
    });
    let create = app
        .clone()
        .oneshot(authed(Method::POST, "/api/settings/ai-profiles", &cookies, Some(payload)))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let created = body_json(create).await;
    assert_eq!(created["name"], "anthropic-main");
    assert_eq!(created["has_secret"], true);
    assert!(!created.to_string().contains("sk-very-secret"));

    let list = app
        .clone()
        .oneshot(authed(Method::GET, "/api/settings/ai-profiles", &cookies, None))
        .await
        .unwrap();
    let listed = body_json(list).await;
    assert_eq!(listed["profiles"].as_array().unwrap().len(), 1);
    assert!(!listed.to_string().contains("sk-very-secret"));
}

#[tokio::test]
async fn invalid_config_is_rejected_on_validate() {
    // A claude_cli profile with a non-existent binary should fail validation
    // cleanly (BadRequest), proving the provider path is wired end to end.
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));
    let cookies = login_cookies(&app, "admin", "admin").await;

    let payload = json!({
        "name": "cli",
        "provider_type": "claude_cli",
        "config": { "binary_path": "definitely-not-a-real-binary-xyz" }
    });
    let create = app
        .clone()
        .oneshot(authed(Method::POST, "/api/settings/ai-profiles", &cookies, Some(payload)))
        .await
        .unwrap();
    let id = body_json(create).await["id"].as_str().unwrap().to_string();

    let validate = app
        .clone()
        .oneshot(authed(
            Method::POST,
            &format!("/api/settings/ai-profiles/{id}/validate"),
            &cookies,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(validate.status(), StatusCode::BAD_REQUEST);
}
