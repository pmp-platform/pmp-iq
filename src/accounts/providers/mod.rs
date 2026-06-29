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
use serde_json::Value;

/// A member/collaborator of a repository, as reported by a provider's API.
#[derive(Debug, Clone)]
pub struct RepoMember {
    pub username: String,
    pub email: Option<String>,
    /// Provider role name (e.g. GitHub `admin`/`write`/`read`).
    pub role: Option<String>,
    /// Raw provider permission flags (free-form JSON).
    pub permissions: Value,
}

/// A pull request as created/looked up via a provider.
#[derive(Debug, Clone)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    pub state: String,
}

/// Parameters to open a pull request.
#[derive(Debug, Clone)]
pub struct PullRequestSpec {
    pub repo_full_name: String,
    pub head_branch: String,
    pub base_branch: String,
    pub title: String,
    pub body: String,
}

/// A PR's reconciliation status (M24).
#[derive(Debug, Clone)]
pub struct PrStatus {
    /// `open` | `closed` | `merged`.
    pub state: String,
    /// `Some(false)` when the PR has merge conflicts; `None` when unknown.
    pub mergeable: Option<bool>,
    pub head_sha: String,
}

/// A comment on a PR.
#[derive(Debug, Clone)]
pub struct PrComment {
    pub id: u64,
    pub author: String,
    pub body: String,
}

/// A CI check / Action result for a PR head commit.
#[derive(Debug, Clone)]
pub struct PrCheck {
    pub name: String,
    pub status: String,
    /// `success` | `failure` | `cancelled` | … (None while pending).
    pub conclusion: Option<String>,
}

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
    #[error("operation not supported by this provider")]
    Unsupported,
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

    /// List the members/collaborators of a repository. Providers without a
    /// member concept (e.g. local) inherit this empty default.
    async fn list_members(&self, _repo_full_name: &str) -> Result<Vec<RepoMember>, ProviderError> {
        Ok(Vec::new())
    }

    /// Open a pull request (or return the existing open one for the same head
    /// branch). Providers that cannot open PRs inherit the `Unsupported` default.
    async fn open_pull_request(&self, _spec: PullRequestSpec) -> Result<PullRequest, ProviderError> {
        Err(ProviderError::Unsupported)
    }

    /// Look up a pull request by number.
    async fn get_pull_request(
        &self,
        _repo_full_name: &str,
        _number: u64,
    ) -> Result<PullRequest, ProviderError> {
        Err(ProviderError::Unsupported)
    }

    // --- PR reconciliation (M24) --------------------------------------------

    /// Open/closed/merged state + mergeability + head SHA of a PR.
    async fn pull_request_status(
        &self,
        _repo_full_name: &str,
        _number: u64,
    ) -> Result<PrStatus, ProviderError> {
        Err(ProviderError::Unsupported)
    }

    /// Comments on a PR (oldest first). Providers without comments return empty.
    async fn pull_request_comments(
        &self,
        _repo_full_name: &str,
        _number: u64,
    ) -> Result<Vec<PrComment>, ProviderError> {
        Ok(Vec::new())
    }

    /// CI checks / Action results for the PR head commit.
    async fn pull_request_checks(
        &self,
        _repo_full_name: &str,
        _head_sha: &str,
    ) -> Result<Vec<PrCheck>, ProviderError> {
        Ok(Vec::new())
    }

    /// Post a comment on a PR.
    async fn post_pull_request_comment(
        &self,
        _repo_full_name: &str,
        _number: u64,
        _body: &str,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFileSystem;
    use crate::httpclient::HttpResponse;
    use std::sync::Arc;

    fn resp_with(header: &str, value: &str) -> HttpResponse {
        let mut resp = HttpResponse::new(429, "");
        resp.headers.insert(header.into(), value.into());
        resp
    }

    #[test]
    fn retry_at_from_relative_and_absolute_headers() {
        assert!(retry_at_from_headers(&resp_with("retry-after", "30")).is_some());
        assert!(retry_at_from_headers(&resp_with("x-ratelimit-reset", "1893456000")).is_some());
        assert!(retry_at_from_headers(&resp_with("ratelimit-reset", "1893456000")).is_some());
        assert!(retry_at_from_headers(&HttpResponse::new(429, "")).is_none());
    }

    #[tokio::test]
    async fn pr_operations_default_to_unsupported() {
        let provider = LocalProvider::new(Arc::new(MockFileSystem::new()), "/repos".into());
        let spec = PullRequestSpec {
            repo_full_name: "org/api".into(),
            head_branch: "h".into(),
            base_branch: "b".into(),
            title: "t".into(),
            body: String::new(),
        };
        assert!(matches!(
            provider.open_pull_request(spec).await,
            Err(ProviderError::Unsupported)
        ));
        assert!(matches!(
            provider.get_pull_request("org/api", 1).await,
            Err(ProviderError::Unsupported)
        ));
    }

    #[test]
    fn provider_error_maps_to_app_error() {
        use crate::error::AppError;
        assert!(matches!(AppError::from(ProviderError::Auth), AppError::BadRequest(_)));
        assert!(matches!(AppError::from(ProviderError::Unsupported), AppError::BadRequest(_)));
        assert!(matches!(
            AppError::from(ProviderError::RateLimited { retry_at: None }),
            AppError::RateLimited { .. }
        ));
    }
}
