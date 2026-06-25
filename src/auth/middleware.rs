//! Session-based authentication middleware.

use super::principal::Principal;
use crate::error::redirect_login;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tower_sessions::Session;

/// Session key under which the authenticated [`Principal`] is stored.
pub const SESSION_PRINCIPAL_KEY: &str = "principal";

/// Reject unauthenticated requests. On success the [`Principal`] is inserted
/// into request extensions so downstream handlers can read it.
///
/// API routes (`/api/...`) receive `401`; page routes are redirected to
/// `/login`.
pub async fn require_auth(session: Session, mut req: Request, next: Next) -> Response {
    match session.get::<Principal>(SESSION_PRINCIPAL_KEY).await {
        Ok(Some(principal)) => {
            req.extensions_mut().insert(principal);
            next.run(req).await
        }
        _ => {
            if req.uri().path().starts_with("/api") {
                StatusCode::UNAUTHORIZED.into_response()
            } else {
                redirect_login()
            }
        }
    }
}
