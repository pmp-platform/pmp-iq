//! Integration test for the global "Ask the platform" route (M26) wiring.
//! (The grounded-answer path is unit-tested with a mocked LLM; here we cover the
//! route's validation/wiring without invoking a real model.)

mod common;

use axum::body::Body;
use axum::http::Request;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use common::{SqliteDb, build_state_sqlite, cookie_header, login_cookies};
use pmp_iq::app::build_router;
use tower::ServiceExt;

async fn post_ask(app: &axum::Router, cookies: &[String], body: &str) -> u16 {
    app.clone()
        .oneshot(
            Request::post("/api/platform/ask")
                .header(CONTENT_TYPE, "application/json")
                .header(COOKIE, cookie_header(cookies))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn ask_platform_without_profile_is_bad_request() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite)); // no AI profile seeded
    let cookies = login_cookies(&app, "admin", "admin").await;
    // The service builds the (empty) catalog graph then finds no profile → 400.
    assert_eq!(post_ask(&app, &cookies, r#"{"question":"which apps exist?"}"#).await, 400);
}

#[tokio::test]
async fn ask_platform_empty_question_is_bad_request() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    let cookies = login_cookies(&app, "admin", "admin").await;
    assert_eq!(post_ask(&app, &cookies, r#"{"question":"   "}"#).await, 400);
}

#[tokio::test]
async fn ask_platform_requires_auth() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(build_state_sqlite(&sqlite));
    // No cookies → unauthenticated API call is rejected.
    let status = app
        .clone()
        .oneshot(
            Request::post("/api/platform/ask")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"question":"x"}"#))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(status, 401);
}
