//! Operator-action audit log (M36): an append-only record of mutating actions
//! (logins, settings/prompt edits, job runs, agent tasks, campaigns) written by
//! [`AuditService`] from the auth layer and mutating routes.

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgPool, SqlitePool};
use std::sync::Arc;
use uuid::Uuid;

/// A persisted audit event.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AuditEvent {
    pub id: Uuid,
    pub actor: String,
    pub action: String,
    pub target: Option<String>,
    pub metadata: Option<Value>,
    pub occurred_at: DateTime<Utc>,
}

/// Read/write the audit log.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait AuditRepository: Send + Sync {
    async fn record(&self, actor: &str, action: &str, target: Option<String>, metadata: Value) -> RepoResult<()>;
    async fn list(&self, limit: i64) -> RepoResult<Vec<AuditEvent>>;
}

const COLS: &str = "id, actor, action, target, metadata, occurred_at";

macro_rules! audit_impl {
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
        impl AuditRepository for $name {
            async fn record(&self, actor: &str, action: &str, target: Option<String>, metadata: Value) -> RepoResult<()> {
                let meta = if metadata.is_null() { None } else { Some(metadata) };
                sqlx::query(&$xform(
                    "INSERT INTO audit_events (id, actor, action, target, metadata) VALUES ($1,$2,$3,$4,$5)",
                ))
                .bind(Uuid::new_v4())
                .bind(actor)
                .bind(action)
                .bind(target)
                .bind(meta)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn list(&self, limit: i64) -> RepoResult<Vec<AuditEvent>> {
                let rows: Vec<AuditEvent> = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM audit_events ORDER BY occurred_at DESC LIMIT $1"
                )))
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows)
            }
        }
    };
}

audit_impl!(PgAuditRepository, PgPool, identity);
audit_impl!(SqliteAuditRepository, SqlitePool, to_sqlite);

/// Records operator actions (best-effort: a logging failure never blocks the
/// action). Cloneable so it can live on [`AppState`].
#[derive(Clone)]
pub struct AuditService {
    repo: Arc<dyn AuditRepository>,
}

impl AuditService {
    pub fn new(repo: Arc<dyn AuditRepository>) -> Self {
        Self { repo }
    }

    /// Record an action; errors are swallowed (auditing must not break the app).
    pub async fn record(&self, actor: &str, action: &str, target: Option<&str>, metadata: Value) {
        let _ = self.repo.record(actor, action, target.map(String::from), metadata).await;
    }

    pub async fn list(&self, limit: i64) -> RepoResult<Vec<AuditEvent>> {
        self.repo.list(limit).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn record_swallows_repo_errors() {
        let mut repo = MockAuditRepository::new();
        repo.expect_record().returning(|_, _, _, _| Err(crate::db::RepoError::NotFound));
        let service = AuditService::new(Arc::new(repo));
        // Must not panic / propagate.
        service.record("admin", "login", None, json!({})).await;
    }

    #[tokio::test]
    async fn record_forwards_fields() {
        let mut repo = MockAuditRepository::new();
        repo.expect_record()
            .withf(|actor, action, target, _| {
                actor == "admin" && action == "settings.update" && target.as_deref() == Some("x")
            })
            .times(1)
            .returning(|_, _, _, _| Ok(()));
        let service = AuditService::new(Arc::new(repo));
        service.record("admin", "settings.update", Some("x"), json!({"k": 1})).await;
    }
}
