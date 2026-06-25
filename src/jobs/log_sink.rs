//! Per-execution log sink.

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use sqlx::{PgPool, SqlitePool};
use uuid::Uuid;

/// Appends progress lines to a job execution.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait LogSink: Send + Sync {
    async fn append(&self, execution_id: Uuid, message: &str) -> RepoResult<()>;
}

macro_rules! log_sink_impl {
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
        impl LogSink for $name {
            async fn append(&self, execution_id: Uuid, message: &str) -> RepoResult<()> {
                let line = format!("{message}\n");
                sqlx::query(&$xform(
                    "UPDATE job_executions SET logs = logs || $2 WHERE id = $1",
                ))
                .bind(execution_id)
                .bind(line)
                .execute(&self.pool)
                .await?;
                Ok(())
            }
        }
    };
}

log_sink_impl!(PgLogSink, PgPool, identity);
log_sink_impl!(SqliteLogSink, SqlitePool, to_sqlite);
