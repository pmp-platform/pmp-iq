//! Git access abstraction so cloning can be unit-tested behind a trait.

use async_trait::async_trait;

/// Request to clone or update a repository (bundled to bound parameters).
#[derive(Debug, Clone)]
pub struct CloneRequest {
    pub clone_url: String,
    pub dest: String,
    pub branch: Option<String>,
    pub token: Option<String>,
}

/// Result of a clone/update.
#[derive(Debug, Clone)]
pub struct CheckoutInfo {
    pub commit_sha: String,
    pub path: String,
}

/// Errors from git operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git error: {0}")]
    Git(String),
}

/// Clones or updates repositories into local working copies.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait GitClient: Send + Sync {
    async fn clone_or_update(&self, request: CloneRequest) -> Result<CheckoutInfo, GitError>;
}

/// `git2`-backed implementation. Blocking git work runs on a blocking thread.
pub struct Git2Client;

impl Git2Client {
    fn credentials_callbacks(token: Option<String>) -> git2::RemoteCallbacks<'static> {
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(move |_url, username, _allowed| match &token {
            Some(t) => git2::Cred::userpass_plaintext(username.unwrap_or("x-access-token"), t),
            None => git2::Cred::default(),
        });
        callbacks
    }

    fn run_clone(request: CloneRequest) -> Result<CheckoutInfo, GitError> {
        let repo = if std::path::Path::new(&request.dest).join(".git").exists() {
            Self::fetch_existing(&request)?
        } else {
            Self::fresh_clone(&request)?
        };
        let sha = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map(|c| c.id().to_string())
            .map_err(|e| GitError::Git(e.to_string()))?;
        Ok(CheckoutInfo {
            commit_sha: sha,
            path: request.dest,
        })
    }

    fn fresh_clone(request: &CloneRequest) -> Result<git2::Repository, GitError> {
        let mut fetch = git2::FetchOptions::new();
        fetch.remote_callbacks(Self::credentials_callbacks(request.token.clone()));
        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(fetch);
        if let Some(branch) = &request.branch {
            builder.branch(branch);
        }
        builder
            .clone(&request.clone_url, std::path::Path::new(&request.dest))
            .map_err(|e| GitError::Git(e.to_string()))
    }

    fn fetch_existing(request: &CloneRequest) -> Result<git2::Repository, GitError> {
        let repo = git2::Repository::open(&request.dest).map_err(|e| GitError::Git(e.to_string()))?;
        {
            let mut remote = repo
                .find_remote("origin")
                .map_err(|e| GitError::Git(e.to_string()))?;
            let mut fetch = git2::FetchOptions::new();
            fetch.remote_callbacks(Self::credentials_callbacks(request.token.clone()));
            remote
                .fetch::<&str>(&[], Some(&mut fetch), None)
                .map_err(|e| GitError::Git(e.to_string()))?;
        }
        Ok(repo)
    }
}

#[async_trait]
impl GitClient for Git2Client {
    async fn clone_or_update(&self, request: CloneRequest) -> Result<CheckoutInfo, GitError> {
        tokio::task::spawn_blocking(move || Self::run_clone(request))
            .await
            .map_err(|e| GitError::Git(e.to_string()))?
    }
}
