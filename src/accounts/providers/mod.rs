//! Repository provider strategies (GitHub, GitLab, local).

mod factory;
mod github;
mod gitlab;
mod local;

pub use factory::{ProviderDeps, RepositoryProviderFactory};
pub use github::GitHubProvider;
pub use gitlab::GitLabProvider;
pub use local::LocalProvider;

use crate::accounts::model::RemoteRepo;
use crate::httpclient::HttpResponse;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Errors raised by repository providers.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("authentication failed")]
    Auth,
    #[error("rate limited by provider")]
    RateLimited { retry_at: Option<DateTime<Utc>> },
    #[error("provider request failed: {0}")]
    Request(String),
    #[error("could not parse provider response: {0}")]
    Parse(String),
    #[error("misconfigured account: {0}")]
    Config(String),
}

impl From<ProviderError> for crate::error::AppError {
    fn from(err: ProviderError) -> Self {
        use crate::error::AppError;
        match err {
            ProviderError::RateLimited { retry_at } => AppError::RateLimited { retry_at },
            ProviderError::Auth => AppError::BadRequest("authentication failed".into()),
            other => AppError::BadRequest(other.to_string()),
        }
    }
}

/// Derive a retry time from common rate-limit headers: `retry-after` (relative
/// seconds) or `x-ratelimit-reset` / `ratelimit-reset` (absolute unix epoch).
pub fn retry_at_from_headers(resp: &HttpResponse) -> Option<DateTime<Utc>> {
    if let Some(secs) = resp.header("retry-after").and_then(|v| v.parse::<i64>().ok()) {
        return Some(Utc::now() + chrono::Duration::seconds(secs));
    }
    let reset = resp
        .header("x-ratelimit-reset")
        .or_else(|| resp.header("ratelimit-reset"))
        .and_then(|v| v.parse::<i64>().ok())?;
    DateTime::from_timestamp(reset, 0)
}

/// A source of repositories for one configured account.
#[async_trait]
pub trait RepositoryProvider: Send + Sync {
    /// Check that the credentials/configuration are usable.
    async fn validate(&self) -> Result<(), ProviderError>;

    /// List all repositories visible to the account (before selection).
    async fn list_repositories(&self) -> Result<Vec<RemoteRepo>, ProviderError>;
}
