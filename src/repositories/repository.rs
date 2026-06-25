//! Data access for repository records.

use super::model::{RepoRecord, RepoRecordInput};
use crate::db::{RepoError, RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use sqlx::{FromRow, PgPool, SqlitePool};
use uuid::Uuid;

/// Access to discovered/cloned repositories.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RepoRecordRepository: Send + Sync {
    /// Insert or update by (account_id, full_name).
    async fn upsert(&self, input: RepoRecordInput) -> RepoResult<RepoRecord>;
    /// Record a successful clone.
    async fn mark_cloned(&self, id: Uuid, local_path: &str, commit_sha: &str) -> RepoResult<()>;
    /// Record that a repository was analysed.
    async fn mark_reviewed(&self, id: Uuid) -> RepoResult<()>;
    async fn get(&self, id: Uuid) -> RepoResult<RepoRecord>;
    async fn list(&self) -> RepoResult<Vec<RepoRecord>>;
}

#[derive(FromRow)]
struct Row {
    id: Uuid,
    account_id: Uuid,
    name: String,
    full_name: String,
    clone_url: String,
    default_branch: Option<String>,
    local_path: Option<String>,
    last_commit_sha: Option<String>,
}

impl From<Row> for RepoRecord {
    fn from(row: Row) -> Self {
        RepoRecord {
            id: row.id,
            account_id: row.account_id,
            name: row.name,
            full_name: row.full_name,
            clone_url: row.clone_url,
            default_branch: row.default_branch,
            local_path: row.local_path,
            last_commit_sha: row.last_commit_sha,
        }
    }
}

const COLS: &str = "id, account_id, name, full_name, clone_url, default_branch, \
     local_path, last_commit_sha";

macro_rules! repo_record_impl {
    ($name:ident, $pool:ty, $xform:path) => {
        pub struct $name {
            pool: $pool,
        }
        impl $name {
            pub fn new(pool: $pool) -> Self {
                Self { pool }
            }
        }
        #[async_trait]
        impl RepoRecordRepository for $name {
            async fn upsert(&self, input: RepoRecordInput) -> RepoResult<RepoRecord> {
                let id = Uuid::new_v4();
                let row: Row = sqlx::query_as(&$xform(
                    "INSERT INTO repositories (id, account_id, name, full_name, clone_url, default_branch) \
                     VALUES ($1,$2,$3,$4,$5,$6) \
                     ON CONFLICT (account_id, full_name) DO UPDATE SET \
                       name = EXCLUDED.name, clone_url = EXCLUDED.clone_url, \
                       default_branch = EXCLUDED.default_branch, updated_at = CURRENT_TIMESTAMP \
                     RETURNING id, account_id, name, full_name, clone_url, default_branch, \
                               local_path, last_commit_sha",
                ))
                .bind(id)
                .bind(input.account_id)
                .bind(&input.name)
                .bind(&input.full_name)
                .bind(&input.clone_url)
                .bind(&input.default_branch)
                .fetch_one(&self.pool)
                .await?;
                Ok(row.into())
            }

            async fn mark_cloned(&self, id: Uuid, local_path: &str, commit_sha: &str) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "UPDATE repositories SET local_path=$2, last_commit_sha=$3, \
                     last_cloned_at=CURRENT_TIMESTAMP, updated_at=CURRENT_TIMESTAMP WHERE id=$1",
                ))
                .bind(id)
                .bind(local_path)
                .bind(commit_sha)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn mark_reviewed(&self, id: Uuid) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "UPDATE repositories SET last_reviewed_at=CURRENT_TIMESTAMP, \
                     updated_at=CURRENT_TIMESTAMP WHERE id=$1",
                ))
                .bind(id)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn get(&self, id: Uuid) -> RepoResult<RepoRecord> {
                let row: Row = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM repositories WHERE id=$1"
                )))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;
                Ok(row.into())
            }

            async fn list(&self) -> RepoResult<Vec<RepoRecord>> {
                let rows: Vec<Row> = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM repositories ORDER BY full_name"
                )))
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(RepoRecord::from).collect())
            }
        }
    };
}

repo_record_impl!(PgRepoRecordRepository, PgPool, identity);
repo_record_impl!(SqliteRepoRecordRepository, SqlitePool, to_sqlite);
