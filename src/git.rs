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

/// Request to commit all working-tree changes in a checkout.
#[derive(Debug, Clone)]
pub struct CommitRequest {
    pub checkout: String,
    pub message: String,
    pub author_name: String,
    pub author_email: String,
}

/// Request to push a branch to `origin` with optional token credentials.
#[derive(Debug, Clone)]
pub struct PushRequest {
    pub checkout: String,
    pub branch: String,
    pub token: Option<String>,
}

/// Errors from git operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git error: {0}")]
    Git(String),
}

/// The files changed between two commits (M41). `base_missing` is true when the
/// `from` commit can't be resolved (force-push/rebase) — the caller then falls
/// back to a full analysis.
#[derive(Debug, Clone, Default)]
pub struct ChangedFiles {
    pub paths: Vec<String>,
    pub base_missing: bool,
}

/// Clones or updates repositories into local working copies.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait GitClient: Send + Sync {
    async fn clone_or_update(&self, request: CloneRequest) -> Result<CheckoutInfo, GitError>;

    /// Clone if missing, then fetch and hard-sync the working tree to the tip of
    /// `origin/<branch>` (equivalent to fetch + rebase for a read-only checkout
    /// with no local commits).
    async fn sync_branch(&self, request: CloneRequest) -> Result<CheckoutInfo, GitError>;

    /// Create (or reset) a local branch at the current HEAD and check it out.
    /// Used by the AI Agent to work on a dedicated `agent/<task>` branch.
    async fn create_branch(&self, checkout: String, branch: String) -> Result<(), GitError>;

    /// Stage all working-tree changes and commit them. Returns `true` when a
    /// commit was created, `false` when there was nothing to commit.
    async fn commit_all(&self, request: CommitRequest) -> Result<bool, GitError>;

    /// Force-push `branch` to `origin` (the agent owns its own branch).
    async fn push_branch(&self, request: PushRequest) -> Result<(), GitError>;

    /// The repo-relative paths that changed between two commits in a checkout
    /// (M41). When `from_sha` is unresolvable, returns `base_missing = true`.
    async fn changed_files(
        &self,
        checkout: String,
        from_sha: String,
        to_sha: String,
    ) -> Result<ChangedFiles, GitError>;
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

    /// Clone if missing, otherwise fetch and hard-reset the working tree to the
    /// tip of `origin/<branch>`.
    fn run_sync(request: CloneRequest) -> Result<CheckoutInfo, GitError> {
        let exists = std::path::Path::new(&request.dest).join(".git").exists();
        let repo = if exists {
            let repo = Self::fetch_existing(&request)?;
            Self::reset_to_remote(&repo, request.branch.as_deref())?;
            repo
        } else {
            Self::fresh_clone(&request)?
        };
        let sha = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map(|c| c.id().to_string())
            .map_err(|e| GitError::Git(e.to_string()))?;
        Ok(CheckoutInfo { commit_sha: sha, path: request.dest })
    }

    fn run_create_branch(checkout: String, branch: String) -> Result<(), GitError> {
        let repo = git2::Repository::open(&checkout).map_err(|e| GitError::Git(e.to_string()))?;
        let commit = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| GitError::Git(e.to_string()))?;
        repo.branch(&branch, &commit, true)
            .map_err(|e| GitError::Git(e.to_string()))?;
        repo.set_head(&format!("refs/heads/{branch}"))
            .map_err(|e| GitError::Git(e.to_string()))?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .map_err(|e| GitError::Git(e.to_string()))?;
        Ok(())
    }

    fn run_commit_all(request: CommitRequest) -> Result<bool, GitError> {
        let repo = git2::Repository::open(&request.checkout)
            .map_err(|e| GitError::Git(e.to_string()))?;
        let mut index = repo.index().map_err(|e| GitError::Git(e.to_string()))?;
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| GitError::Git(e.to_string()))?;
        index.write().map_err(|e| GitError::Git(e.to_string()))?;
        let tree_oid = index.write_tree().map_err(|e| GitError::Git(e.to_string()))?;
        let parent = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| GitError::Git(e.to_string()))?;
        // Nothing changed → no commit.
        if parent.tree().map(|t| t.id()) == Ok(tree_oid) {
            return Ok(false);
        }
        let tree = repo.find_tree(tree_oid).map_err(|e| GitError::Git(e.to_string()))?;
        let sig = git2::Signature::now(&request.author_name, &request.author_email)
            .map_err(|e| GitError::Git(e.to_string()))?;
        repo.commit(Some("HEAD"), &sig, &sig, &request.message, &tree, &[&parent])
            .map_err(|e| GitError::Git(e.to_string()))?;
        Ok(true)
    }

    fn run_push(request: PushRequest) -> Result<(), GitError> {
        let repo = git2::Repository::open(&request.checkout)
            .map_err(|e| GitError::Git(e.to_string()))?;
        let mut remote = repo
            .find_remote("origin")
            .map_err(|e| GitError::Git(e.to_string()))?;
        let mut opts = git2::PushOptions::new();
        opts.remote_callbacks(Self::credentials_callbacks(request.token));
        let refspec = format!("+refs/heads/{0}:refs/heads/{0}", request.branch);
        remote
            .push(&[refspec.as_str()], Some(&mut opts))
            .map_err(|e| GitError::Git(e.to_string()))?;
        Ok(())
    }

    /// Hard-reset the checkout to `origin/<branch>` (detached) after a fetch.
    fn reset_to_remote(repo: &git2::Repository, branch: Option<&str>) -> Result<(), GitError> {
        let branch = branch.unwrap_or("main");
        let refname = format!("refs/remotes/origin/{branch}");
        let oid = repo
            .refname_to_id(&refname)
            .map_err(|e| GitError::Git(format!("branch '{branch}' not found: {e}")))?;
        let object = repo
            .find_object(oid, None)
            .map_err(|e| GitError::Git(e.to_string()))?;
        repo.reset(&object, git2::ResetType::Hard, None)
            .map_err(|e| GitError::Git(e.to_string()))?;
        repo.set_head_detached(oid).map_err(|e| GitError::Git(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl GitClient for Git2Client {
    async fn clone_or_update(&self, request: CloneRequest) -> Result<CheckoutInfo, GitError> {
        tokio::task::spawn_blocking(move || Self::run_clone(request))
            .await
            .map_err(|e| GitError::Git(e.to_string()))?
    }

    async fn sync_branch(&self, request: CloneRequest) -> Result<CheckoutInfo, GitError> {
        tokio::task::spawn_blocking(move || Self::run_sync(request))
            .await
            .map_err(|e| GitError::Git(e.to_string()))?
    }

    async fn create_branch(&self, checkout: String, branch: String) -> Result<(), GitError> {
        tokio::task::spawn_blocking(move || Self::run_create_branch(checkout, branch))
            .await
            .map_err(|e| GitError::Git(e.to_string()))?
    }

    async fn commit_all(&self, request: CommitRequest) -> Result<bool, GitError> {
        tokio::task::spawn_blocking(move || Self::run_commit_all(request))
            .await
            .map_err(|e| GitError::Git(e.to_string()))?
    }

    async fn push_branch(&self, request: PushRequest) -> Result<(), GitError> {
        tokio::task::spawn_blocking(move || Self::run_push(request))
            .await
            .map_err(|e| GitError::Git(e.to_string()))?
    }

    async fn changed_files(
        &self,
        checkout: String,
        from_sha: String,
        to_sha: String,
    ) -> Result<ChangedFiles, GitError> {
        tokio::task::spawn_blocking(move || Self::run_changed_files(checkout, from_sha, to_sha))
            .await
            .map_err(|e| GitError::Git(e.to_string()))?
    }
}

impl Git2Client {
    /// Diff two commit trees and collect the changed repo-relative paths. An
    /// unresolvable `from_sha` (force-push/rebase) returns `base_missing`.
    fn run_changed_files(checkout: String, from_sha: String, to_sha: String) -> Result<ChangedFiles, GitError> {
        let repo = git2::Repository::open(&checkout).map_err(|e| GitError::Git(e.to_string()))?;
        let from_tree = match repo.revparse_single(&from_sha).and_then(|o| o.peel_to_tree()) {
            Ok(tree) => tree,
            // Base commit gone (history rewritten) → signal a full re-analysis.
            Err(_) => return Ok(ChangedFiles { paths: vec![], base_missing: true }),
        };
        let to_tree = repo
            .revparse_single(&to_sha)
            .and_then(|o| o.peel_to_tree())
            .map_err(|e| GitError::Git(format!("target commit '{to_sha}' not found: {e}")))?;
        let diff = repo
            .diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)
            .map_err(|e| GitError::Git(e.to_string()))?;
        let mut paths = std::collections::BTreeSet::new();
        for delta in diff.deltas() {
            if let Some(p) = delta.new_file().path().or_else(|| delta.old_file().path()) {
                paths.insert(p.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(ChangedFiles { paths: paths.into_iter().collect(), base_missing: false })
    }
}
