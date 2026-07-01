//! HTTP route composition. Individual feature route modules are merged here.

use crate::app::AppState;
use crate::auth::require_auth;
use axum::Router;
use axum::middleware::from_fn;
use time::Duration;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};

pub mod ai_profiles;
pub mod analysis_config;
pub mod auth;
pub mod cost;
pub mod dora;
pub mod gamification;
pub mod health;
pub mod jobs;
pub mod pages;
pub mod platform;
pub mod rbac;
pub mod remediation;
pub mod scorecards;
pub mod search;
pub mod settings;
pub mod techradar;
pub mod timeline;
pub mod trends;
pub mod webhooks;

/// Routes reachable without authentication.
fn public_routes() -> Router<AppState> {
    Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(webhooks::routes())
}

/// Routes that require an authenticated session.
fn protected_routes() -> Router<AppState> {
    Router::new()
        .merge(pages::routes())
        .merge(settings::routes())
        .merge(ai_profiles::routes())
        .merge(analysis_config::routes())
        .merge(jobs::routes())
        .merge(platform::routes())
        .merge(cost::routes())
        .merge(search::routes())
        .merge(timeline::routes())
        .merge(rbac::routes())
        .merge(scorecards::routes())
        .merge(gamification::routes())
        .merge(techradar::routes())
        .merge(remediation::routes())
        .merge(dora::routes())
        .merge(trends::routes())
        .route_layer(from_fn(require_auth))
}

/// Build the application router from all feature route modules.
pub fn router(state: AppState) -> Router {
    let assets_dir = state.config.server.assets_dir.clone();
    let session_layer = SessionManagerLayer::new(MemoryStore::default())
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_secure(false)
        .with_expiry(Expiry::OnInactivity(Duration::hours(8)));

    Router::new()
        .merge(public_routes())
        .merge(protected_routes())
        .nest_service("/assets", ServeDir::new(assets_dir))
        .layer(session_layer)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
