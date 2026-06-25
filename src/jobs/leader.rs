//! Distributed lock used for leader election among multiple instances.

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, SqlitePool};

/// A TTL-based advisory lock backed by the `controller_locks` table.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait LeaderLock: Send + Sync {
    /// Try to acquire or renew the named lock for `holder` until `expires_at`.
    /// Succeeds if the lock is free, already held by `holder`, or expired as of
    /// `now`. Returns whether the lock is now held by `holder`.
    async fn try_acquire(
        &self,
        name: &str,
        holder: &str,
        expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> RepoResult<bool>;
}

macro_rules! leader_lock_impl {
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
        impl LeaderLock for $name {
            async fn try_acquire(
                &self,
                name: &str,
                holder: &str,
                expires_at: DateTime<Utc>,
                now: DateTime<Utc>,
            ) -> RepoResult<bool> {
                let res = sqlx::query(&$xform(
                    "INSERT INTO controller_locks (name, holder, expires_at) VALUES ($1,$2,$3) \
                     ON CONFLICT (name) DO UPDATE SET holder=$2, expires_at=$3 \
                     WHERE controller_locks.holder=$2 OR controller_locks.expires_at < $4",
                ))
                .bind(name)
                .bind(holder)
                .bind(expires_at)
                .bind(now)
                .execute(&self.pool)
                .await?;
                Ok(res.rows_affected() > 0)
            }
        }
    };
}

leader_lock_impl!(PgLeaderLock, PgPool, identity);
leader_lock_impl!(SqliteLeaderLock, SqlitePool, to_sqlite);
