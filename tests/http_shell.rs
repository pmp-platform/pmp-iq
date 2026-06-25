//! Integration test for the HTTP shell: public routes, the auth gate, and that
//! vendored assets are served locally.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestDb, build_state};
use http_body_util::BodyExt;
use platform_inspector::app::build_router;
use tower::ServiceExt;

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn protected_page_redirects_when_unauthenticated() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));

    let resp = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(resp.headers().get("location").unwrap(), "/login");
}

#[tokio::test]
async fn login_page_renders_locally_vendored_assets() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));

    let resp = app
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("Sign in"));
    assert!(html.contains("/assets/vendor/jquery.min.js"));
    assert!(html.contains("/assets/vendor/tailwind.js"));
    assert!(!html.contains("https://cdn"));
    assert!(!html.contains("code.jquery.com"));
}

#[tokio::test]
async fn healthz_is_public() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));

    let resp = app
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_string(resp).await.contains("ok"));
}

#[tokio::test]
async fn vendored_assets_are_served() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));

    let resp = app
        .oneshot(Request::get("/assets/vendor/jquery.min.js").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_string(resp).await.contains("jQuery"));
}
