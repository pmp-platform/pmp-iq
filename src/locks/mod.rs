//! Distributed locks: a named lock with a TTL that can be refreshed.
//!
//! The [`DistributedLock`] trait lets any subsystem serialise work by an
//! arbitrary key. [`InMemoryLock`] is the simple, process-local first backend
//! (correct for the default single-instance / SQLite deployment); the SQL-backed
//! backend over `controller_locks` keeps multi-instance deployments correct.

pub mod keys;
pub mod memory;
pub mod redis;
pub mod sql;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;

pub use keys as lock_keys;
pub use memory::InMemoryLock;
pub use redis::{Op, RedisClient, RedisClientImpl, RedisLock};
pub use sql::{PgSqlLock, SqliteSqlLock};

/// A granted lease. Carries the holder token used to refresh/release it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub key: String,
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

/// Errors raised by lock backends.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// The lease is no longer held by this token (taken over or expired).
    #[error("lock '{0}' lost")]
    Lost(String),
    /// The lock backend failed.
    #[error("lock backend error: {0}")]
    Backend(String),
}

pub type LockResult<T> = Result<T, LockError>;

/// A TTL-based distributed lock.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait DistributedLock: Send + Sync {
    /// Try to take `key` for `ttl`. Returns `Some(Lease)` when granted (the key
    /// is free or its prior lease has expired), `None` when it is currently held
    /// by someone else.
    async fn acquire(&self, key: &str, ttl: Duration) -> LockResult<Option<Lease>>;

    /// Extend an existing lease by `ttl` from now. Errors with [`LockError::Lost`]
    /// when the lease is no longer held by this token.
    async fn refresh(&self, lease: &Lease, ttl: Duration) -> LockResult<Lease>;

    /// Release the lease (a no-op when it is already lost).
    async fn release(&self, lease: &Lease) -> LockResult<()>;
}

/// Convert a std `Duration` into a chrono `Duration`, clamping on overflow.
pub(crate) fn chrono_ttl(ttl: Duration) -> chrono::Duration {
    chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::zero())
}
