//! Model for cloned/discovered repositories.

use serde::Serialize;
use uuid::Uuid;

/// A repository discovered and (optionally) cloned for a configured account.
#[derive(Debug, Clone, Serialize)]
pub struct RepoRecord {
    pub id: Uuid,
    pub account_id: Uuid,
    pub name: String,
    pub full_name: String,
    pub clone_url: String,
    pub default_branch: Option<String>,
    pub local_path: Option<String>,
    pub last_commit_sha: Option<String>,
    /// The commit last successfully analyzed (M41); `None` until a first sync.
    pub last_analyzed_sha: Option<String>,
}

/// Fields needed to upsert a repository record.
#[derive(Debug, Clone)]
pub struct RepoRecordInput {
    pub account_id: Uuid,
    pub name: String,
    pub full_name: String,
    pub clone_url: String,
    pub default_branch: Option<String>,
}
