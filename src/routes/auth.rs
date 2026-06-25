//! Login / logout routes with per-session CSRF protection.

use crate::app::AppState;
use crate::auth::principal::Credentials;
use crate::auth::{SESSION_PRINCIPAL_KEY, RandomSecretGenerator, SecretGenerator};
use crate::error::{AppError, AppResult};
use crate::web::{PageContext, render_page};
use axum::Form;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;

const CSRF_KEY: &str = "csrf";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_form).post(login_submit))
        .route("/logout", post(logout))
}

/// Submitted login form.
#[derive(Deserialize)]
struct LoginForm {
    csrf: String,
    username: String,
    password: String,
}

/// Fetch the session CSRF token, generating and storing one if absent.
async fn ensure_csrf(session: &Session) -> AppResult<String> {
    if let Some(token) = session
        .get::<String>(CSRF_KEY)
        .await
        .map_err(AppError::internal)?
    {
        return Ok(token);
    }
    let token = RandomSecretGenerator.generate(32);
    session
        .insert(CSRF_KEY, &token)
        .await
        .map_err(AppError::internal)?;
    Ok(token)
}

async fn render_login(state: &AppState, csrf: &str, error: Option<&str>) -> AppResult<Html<String>> {
    let page = PageContext::new(None, "login");
    render_page(
        &state.engine,
        "login.html",
        &page,
        context! { csrf => csrf, error => error },
    )
}

async fn login_form(State(state): State<AppState>, session: Session) -> AppResult<Html<String>> {
    let csrf = ensure_csrf(&session).await?;
    render_login(&state, &csrf, None).await
}

async fn login_submit(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<LoginForm>,
) -> AppResult<Response> {
    let expected = ensure_csrf(&session).await?;
    if form.csrf != expected {
        return Err(AppError::BadRequest("invalid CSRF token".into()));
    }

    let creds = Credentials {
        username: form.username,
        password: form.password,
    };
    match state.auth.authenticate(&creds).await {
        Ok(principal) => {
            session
                .insert(SESSION_PRINCIPAL_KEY, &principal)
                .await
                .map_err(AppError::internal)?;
            Ok(Redirect::to("/").into_response())
        }
        Err(_) => {
            let html = render_login(&state, &expected, Some("Invalid username or password")).await?;
            Ok((axum::http::StatusCode::UNAUTHORIZED, html).into_response())
        }
    }
}

async fn logout(session: Session) -> AppResult<Redirect> {
    session.flush().await.map_err(AppError::internal)?;
    Ok(Redirect::to("/login"))
}
