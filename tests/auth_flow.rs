//! End-to-end authentication flow against a real session layer.

mod common;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, LOCATION};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use common::{TestDb, build_state, cookie_header, extract_cookies, extract_csrf};
use http_body_util::BodyExt;
use platform_inspector::app::build_router;
use tower::ServiceExt;
use axum::Router;

async fn body_string(resp: Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn form_body(pairs: &[(&str, &str)]) -> String {
    serde_urlencoded::to_string(pairs).unwrap()
}

/// Perform the GET /login + POST /login dance, returning the post response and
/// the session cookies to reuse.
async fn login(app: &Router, username: &str, password: &str) -> (Response, Vec<String>) {
    let get = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cookies = extract_cookies(&get);
    let html = body_string(get).await;
    let csrf = extract_csrf(&html);

    let body = form_body(&[("csrf", &csrf), ("username", username), ("password", password)]);
    let resp = app
        .clone()
        .oneshot(
            Request::post("/login")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let post_cookies = extract_cookies(&resp);
    let merged = if post_cookies.is_empty() { cookies } else { post_cookies };
    (resp, merged)
}

#[tokio::test]
async fn successful_login_grants_access() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));

    let (resp, cookies) = login(&app, "admin", "admin").await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(resp.headers().get(LOCATION).unwrap(), "/");

    let home = app
        .oneshot(
            Request::get("/")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(home.status(), StatusCode::OK);
    assert!(body_string(home).await.contains("Dashboard"));
}

#[tokio::test]
async fn wrong_password_is_rejected() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));

    let (resp, _) = login(&app, "admin", "wrong").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(body_string(resp).await.contains("Invalid username or password"));
}

#[tokio::test]
async fn missing_csrf_is_rejected() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));

    // Establish a session, then post with a bogus CSRF token.
    let get = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cookies = extract_cookies(&get);
    let _ = body_string(get).await;

    let body = form_body(&[("csrf", "bogus"), ("username", "admin"), ("password", "admin")]);
    let resp = app
        .oneshot(
            Request::post("/login")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn logout_clears_session() {
    let db = TestDb::start().await;
    let app = build_router(build_state(&db));

    let (_, cookies) = login(&app, "admin", "admin").await;
    let logout = app
        .clone()
        .oneshot(
            Request::post("/logout")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::SEE_OTHER);

    // After logout the same cookie no longer grants access.
    let after = app
        .oneshot(
            Request::get("/")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(after.status(), StatusCode::SEE_OTHER);
    assert_eq!(after.headers().get(LOCATION).unwrap(), "/login");
}
