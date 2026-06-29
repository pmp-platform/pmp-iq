//! Integration test for GitHub authentication: the OAuth web flow
//! (`/auth/github/login` → `/auth/github/callback`) and the personal-token form
//! path, driven through the real routers with a fake `GitHubIdentity`.

mod common;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, LOCATION, SET_COOKIE};
use axum::http::{Request, StatusCode};
use common::{SqliteDb, cookie_header, extract_cookies};
use platiq::app::{AppState, build_router};
use platiq::auth::{
    Argon2Hasher, AuthError, AuthService, GitHubIdentity, GitHubUser, OAuthExchange,
    RandomSecretGenerator,
};
use platiq::config::{Config, MapEnv};
use platiq::db::Database;
use std::sync::Arc;
use tower::ServiceExt;

/// A `GitHubIdentity` that always reports the same login and exchanges any code
/// for a fixed token.
struct FakeIdentity {
    login: String,
}

#[async_trait]
impl GitHubIdentity for FakeIdentity {
    async fn current_user(&self, _token: &str) -> Result<GitHubUser, AuthError> {
        Ok(GitHubUser { login: self.login.clone(), id: 1 })
    }
    async fn user_orgs(&self, _token: &str) -> Result<Vec<String>, AuthError> {
        Ok(vec![])
    }
    async fn exchange_code(&self, _exchange: OAuthExchange) -> Result<String, AuthError> {
        Ok("gho_fixed_token".into())
    }
}

/// Build a GitHub-auth `AppState` with the given mode, allowlisted login, and a
/// fake identity reporting `actual_login`.
fn github_state(db: Database, mode: &str, allowed: &str, actual_login: &str) -> AppState {
    let workspace = std::env::temp_dir()
        .join(format!("pi-gh-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let env = MapEnv::new()
        .with("AUTH_PROVIDER", "github")
        .with("GITHUB_AUTH_MODE", mode)
        .with("GITHUB_CLIENT_ID", "cid")
        .with("GITHUB_CLIENT_SECRET", "secret")
        .with("GITHUB_REDIRECT_URL", "http://localhost:8080/auth/github/callback")
        .with("GITHUB_ALLOWED_LOGINS", allowed)
        .with("WORKSPACE_DIR", &workspace);
    let config = Config::load(&env).unwrap();
    let identity: Arc<dyn GitHubIdentity> = Arc::new(FakeIdentity { login: actual_login.into() });
    let boot = AuthService::from_config(
        &config.auth,
        Arc::new(Argon2Hasher),
        &RandomSecretGenerator,
        Some(identity.clone()),
    )
    .unwrap();
    AppState::build(config, db, Arc::new(boot.service), Some(identity)).unwrap()
}

fn state_from_location(location: &str) -> String {
    location.split("state=").nth(1).unwrap().to_string()
}

#[tokio::test]
async fn oauth_login_redirects_then_callback_authenticates() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(github_state(sqlite.database(), "oauth_app", "octocat", "octocat"));

    // 1) Start the flow: redirect to GitHub's authorize URL with a CSRF state.
    let start = app
        .clone()
        .oneshot(Request::get("/auth/github/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::SEE_OTHER);
    let cookies = extract_cookies(&start);
    let location = start.headers().get(LOCATION).unwrap().to_str().unwrap().to_string();
    assert!(location.contains("/login/oauth/authorize"));
    assert!(location.contains("client_id=cid"));
    let state = state_from_location(&location);

    // 2) GitHub redirects back with code + the same state; we authenticate.
    let cb = app
        .clone()
        .oneshot(
            Request::get(format!("/auth/github/callback?code=abc&state={state}"))
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cb.status(), StatusCode::SEE_OTHER, "callback signs in");
    assert_eq!(cb.headers().get(LOCATION).unwrap().to_str().unwrap(), "/");
    // A session principal cookie is issued.
    assert!(cb.headers().get_all(SET_COOKIE).iter().count() > 0);
}

#[tokio::test]
async fn oauth_callback_denies_non_allowlisted_user() {
    let sqlite = SqliteDb::start().await;
    // The fake identity reports "intruder", who is not in the allowlist.
    let app = build_router(github_state(sqlite.database(), "oauth_app", "octocat", "intruder"));

    let start = app
        .clone()
        .oneshot(Request::get("/auth/github/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cookies = extract_cookies(&start);
    let state = state_from_location(start.headers().get(LOCATION).unwrap().to_str().unwrap());

    let cb = app
        .clone()
        .oneshot(
            Request::get(format!("/auth/github/callback?code=abc&state={state}"))
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cb.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn oauth_callback_rejects_bad_state() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(github_state(sqlite.database(), "oauth_app", "octocat", "octocat"));
    let start = app
        .clone()
        .oneshot(Request::get("/auth/github/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cookies = extract_cookies(&start);
    let cb = app
        .clone()
        .oneshot(
            Request::get("/auth/github/callback?code=abc&state=WRONG")
                .header(COOKIE, cookie_header(&cookies))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cb.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn personal_token_form_login_authenticates_allowlisted_user() {
    let sqlite = SqliteDb::start().await;
    let app = build_router(github_state(sqlite.database(), "personal_token", "octocat", "octocat"));

    // GET /login to obtain a CSRF token + session cookie.
    let get = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cookies = extract_cookies(&get);
    let html = {
        use http_body_util::BodyExt;
        let bytes = get.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    };
    let csrf = common::extract_csrf(&html);

    let body = serde_urlencoded::to_string([
        ("csrf", csrf.as_str()),
        ("username", "octocat"),
        ("password", "ghp_a_token"),
    ])
    .unwrap();
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
    assert_eq!(resp.status(), StatusCode::SEE_OTHER, "token login signs in");
    assert_eq!(resp.headers().get(LOCATION).unwrap().to_str().unwrap(), "/");
}
