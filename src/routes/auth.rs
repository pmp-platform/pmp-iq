//! Login / logout routes with per-session CSRF protection, plus the GitHub
//! OAuth web flow (M21).

use crate::app::{AppState, GitHubAuthState};
use crate::auth::principal::Credentials;
use crate::auth::{
    OAuthExchange, Principal, RandomSecretGenerator, SESSION_PRINCIPAL_KEY, SecretGenerator,
    authorize,
};
use crate::config::{AuthProvider, GitHubAuthMode};
use crate::error::{AppError, AppResult};
use crate::strings::percent_encode;
use crate::web::{PageContext, render_page};
use axum::Form;
use axum::Router;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use minijinja::context;
use serde::Deserialize;
use tower_sessions::Session;

const CSRF_KEY: &str = "csrf";
const GH_STATE_KEY: &str = "gh_oauth_state";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_form).post(login_submit))
        .route("/logout", post(logout))
        .route("/auth/github/login", get(github_login))
        .route("/auth/github/callback", get(github_callback))
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

/// The login provider + GitHub mode exposed to the template so it renders the
/// right form (password / GitHub button / token field).
fn login_provider(state: &AppState) -> (&'static str, &'static str) {
    match state.config.auth.provider {
        AuthProvider::Github => {
            let mode = match state.config.auth.github.as_ref().map(|g| g.mode) {
                Some(GitHubAuthMode::PersonalToken) => "personal_token",
                _ => "oauth_app",
            };
            ("github", mode)
        }
        AuthProvider::Admin => ("admin", ""),
    }
}

async fn render_login(state: &AppState, csrf: &str, error: Option<&str>) -> AppResult<Html<String>> {
    let page = PageContext::new(None, "login");
    let (provider, github_mode) = login_provider(state);
    render_page(
        &state.engine,
        "login.html",
        &page,
        context! {
            csrf => csrf,
            error => error,
            auth_provider => provider,
            github_mode => github_mode,
        },
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

// --- GitHub OAuth web flow --------------------------------------------------

/// Start the OAuth flow: store a CSRF `state` and redirect to GitHub.
async fn github_login(State(state): State<AppState>, session: Session) -> AppResult<Response> {
    let gh = state
        .github_auth
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("GitHub auth is not configured".into()))?;
    let client_id = gh
        .config
        .client_id
        .as_deref()
        .ok_or_else(|| AppError::internal("GitHub client_id is not configured"))?;
    let redirect = gh
        .config
        .redirect_url
        .as_deref()
        .ok_or_else(|| AppError::internal("GitHub redirect_url is not configured"))?;
    let csrf = RandomSecretGenerator.generate(32);
    session
        .insert(GH_STATE_KEY, &csrf)
        .await
        .map_err(AppError::internal)?;
    let url = format!(
        "{}/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&state={}",
        gh.config.web_base_url,
        percent_encode(client_id),
        percent_encode(redirect),
        percent_encode("read:user read:org"),
        percent_encode(&csrf),
    );
    Ok(Redirect::to(&url).into_response())
}

/// GitHub redirects back here with `code` + `state`.
#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

async fn github_callback(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<CallbackQuery>,
) -> AppResult<Response> {
    let gh = match state.github_auth.as_ref() {
        Some(gh) => gh,
        None => return Err(AppError::BadRequest("GitHub auth is not configured".into())),
    };
    let expected: Option<String> = session.get(GH_STATE_KEY).await.map_err(AppError::internal)?;
    let _ = session.remove::<String>(GH_STATE_KEY).await; // one-time use

    if expected.is_none() || query.state.is_none() || query.state != expected {
        return denied(&state, &session, "Invalid sign-in state — please try again").await;
    }
    let code = match query.code {
        Some(code) => code,
        None => return denied(&state, &session, "GitHub sign-in was cancelled").await,
    };
    match resolve_principal(gh, code).await {
        Some(principal) => {
            session
                .insert(SESSION_PRINCIPAL_KEY, &principal)
                .await
                .map_err(AppError::internal)?;
            Ok(Redirect::to("/").into_response())
        }
        None => denied(&state, &session, "Access denied for this GitHub account").await,
    }
}

/// Exchange the code, verify the user, and apply the allowlist. `None` on any
/// failure or denial (details are not surfaced to the user).
async fn resolve_principal(gh: &GitHubAuthState, code: String) -> Option<Principal> {
    let token = gh
        .identity
        .exchange_code(OAuthExchange {
            client_id: gh.config.client_id.clone().unwrap_or_default(),
            client_secret: gh.config.client_secret.clone().unwrap_or_default(),
            code,
            redirect_url: gh.config.redirect_url.clone().unwrap_or_default(),
        })
        .await
        .ok()?;
    let user = gh.identity.current_user(&token).await.ok()?;
    let orgs = gh.identity.user_orgs(&token).await.ok()?;
    authorize(&user, &orgs, &gh.config).then(|| Principal::github(&user.login))
}

/// Render the login page with an access-denied message (HTTP 401).
async fn denied(state: &AppState, session: &Session, message: &str) -> AppResult<Response> {
    let csrf = ensure_csrf(session).await?;
    let html = render_login(state, &csrf, Some(message)).await?;
    Ok((axum::http::StatusCode::UNAUTHORIZED, html).into_response())
}
