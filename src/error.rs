//! Application-wide error type and HTTP mapping.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use serde_json::json;

/// The single error type returned by handlers and services.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("rate limited")]
    RateLimited {
        retry_at: Option<chrono::DateTime<chrono::Utc>>,
    },

    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn status(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Build an internal error from any displayable error.
    pub fn internal(err: impl std::fmt::Display) -> Self {
        AppError::Internal(err.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Json(json!({
            "error": {
                "code": status.as_u16(),
                "message": self.to_string(),
            }
        }));
        (status, body).into_response()
    }
}

/// Result alias used throughout the application.
pub type AppResult<T> = Result<T, AppError>;

/// Helper used by page handlers: redirect to login on auth failure instead of
/// returning JSON.
pub fn redirect_login() -> Response {
    Redirect::to("/login").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_map_correctly() {
        assert_eq!(AppError::NotFound("x".into()).status(), StatusCode::NOT_FOUND);
        assert_eq!(AppError::BadRequest("x".into()).status(), StatusCode::BAD_REQUEST);
        assert_eq!(AppError::Unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(AppError::Conflict("x".into()).status(), StatusCode::CONFLICT);
        assert_eq!(AppError::Internal("x".into()).status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn internal_wraps_display() {
        let err = AppError::internal("boom");
        assert!(matches!(err, AppError::Internal(ref m) if m == "boom"));
    }
}
