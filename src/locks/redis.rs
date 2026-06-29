//! Redis-backed distributed lock. Coordinates locks across instances through a
//! shared Redis, using the classic single-instance Redlock primitives:
//! `SET key token NX PX ttl` to acquire and token-checked Lua scripts to refresh
//! (`PEXPIRE`) and release (`DEL`). The Redis I/O sits behind [`RedisClient`] so
//! the lock algorithm is unit-tested without a real server.

use super::{DistributedLock, Lease, LockError, LockResult, chrono_ttl};
use crate::jobs::clock::Clock;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// A token-checked operation run by [`RedisClient::run_if_owner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Extend the key's TTL to this many milliseconds (refresh).
    Pexpire(u64),
    /// Delete the key (release).
    Del,
}

/// The minimal Redis surface the lock needs. Mocked in tests.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RedisClient: Send + Sync {
    /// `SET key token NX PX ttl_ms` — returns `true` when the key was set.
    async fn set_nx_px(&self, key: &str, token: &str, ttl_ms: u64) -> LockResult<bool>;

    /// Run `op` only when `GET key == token` (atomic Lua). Returns whether the
    /// script matched the token (i.e. we still hold the lease).
    async fn run_if_owner(&self, key: &str, token: &str, op: Op) -> LockResult<bool>;
}

const REFRESH_LUA: &str = "if redis.call('get', KEYS[1]) == ARGV[1] then \
    return redis.call('pexpire', KEYS[1], ARGV[2]) else return 0 end";
const RELEASE_LUA: &str = "if redis.call('get', KEYS[1]) == ARGV[1] then \
    return redis.call('del', KEYS[1]) else return 0 end";

fn backend(e: impl std::fmt::Display) -> LockError {
    LockError::Backend(e.to_string())
}

/// `RedisClient` over the `redis` crate, with a lazily-established multiplexed
/// connection shared across operations.
pub struct RedisClientImpl {
    client: ::redis::Client,
    conn: tokio::sync::OnceCell<::redis::aio::MultiplexedConnection>,
}

impl RedisClientImpl {
    /// Parse the connection URL (no I/O yet); the connection is established on
    /// first use. An invalid URL fails fast here.
    pub fn connect(url: &str) -> LockResult<Self> {
        let client = ::redis::Client::open(url).map_err(backend)?;
        Ok(Self {
            client,
            conn: tokio::sync::OnceCell::new(),
        })
    }

    async fn conn(&self) -> LockResult<::redis::aio::MultiplexedConnection> {
        let conn = self
            .conn
            .get_or_try_init(|| self.client.get_multiplexed_async_connection())
            .await
            .map_err(backend)?;
        Ok(conn.clone())
    }
}

#[async_trait]
impl RedisClient for RedisClientImpl {
    async fn set_nx_px(&self, key: &str, token: &str, ttl_ms: u64) -> LockResult<bool> {
        let mut conn = self.conn().await?;
        let res: Option<String> = ::redis::cmd("SET")
            .arg(key)
            .arg(token)
            .arg("NX")
            .arg("PX")
            .arg(ttl_ms)
            .query_async(&mut conn)
            .await
            .map_err(backend)?;
        Ok(res.is_some())
    }

    async fn run_if_owner(&self, key: &str, token: &str, op: Op) -> LockResult<bool> {
        let mut conn = self.conn().await?;
        let script = ::redis::Script::new(match op {
            Op::Pexpire(_) => REFRESH_LUA,
            Op::Del => RELEASE_LUA,
        });
        let mut invocation = script.key(key);
        invocation.arg(token);
        if let Op::Pexpire(ttl_ms) = op {
            invocation.arg(ttl_ms);
        }
        let changed: i64 = invocation.invoke_async(&mut conn).await.map_err(backend)?;
        Ok(changed == 1)
    }
}

/// A TTL distributed lock backed by Redis. Correct across instances sharing the
/// same Redis. Time comes from the injected [`Clock`] so `expires_at` is
/// deterministic in tests.
pub struct RedisLock {
    client: Arc<dyn RedisClient>,
    clock: Arc<dyn Clock>,
    prefix: String,
}

impl RedisLock {
    pub fn new(client: Arc<dyn RedisClient>, clock: Arc<dyn Clock>) -> Self {
        Self {
            client,
            clock,
            prefix: "pi:lock:".to_string(),
        }
    }

    /// Namespace the key so locks don't collide with other keys in a shared
    /// Redis.
    fn namespaced(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key)
    }
}

#[async_trait]
impl DistributedLock for RedisLock {
    async fn acquire(&self, key: &str, ttl: Duration) -> LockResult<Option<Lease>> {
        let token = Uuid::new_v4().to_string();
        let ttl_ms = ttl.as_millis() as u64;
        if self.client.set_nx_px(&self.namespaced(key), &token, ttl_ms).await? {
            let expires_at = self.clock.now() + chrono_ttl(ttl);
            Ok(Some(Lease { key: key.to_string(), token, expires_at }))
        } else {
            Ok(None)
        }
    }

    async fn refresh(&self, lease: &Lease, ttl: Duration) -> LockResult<Lease> {
        let ttl_ms = ttl.as_millis() as u64;
        let held = self
            .client
            .run_if_owner(&self.namespaced(&lease.key), &lease.token, Op::Pexpire(ttl_ms))
            .await?;
        if held {
            Ok(Lease {
                key: lease.key.clone(),
                token: lease.token.clone(),
                expires_at: self.clock.now() + chrono_ttl(ttl),
            })
        } else {
            Err(LockError::Lost(lease.key.clone()))
        }
    }

    async fn release(&self, lease: &Lease) -> LockResult<()> {
        self.client
            .run_if_owner(&self.namespaced(&lease.key), &lease.token, Op::Del)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::clock::MockClock;
    use chrono::{TimeZone, Utc};
    use mockall::predicate::*;

    fn fixed_clock(secs: i64) -> Arc<MockClock> {
        let mut clock = MockClock::new();
        clock.expect_now().returning(move || Utc.timestamp_opt(secs, 0).unwrap());
        Arc::new(clock)
    }

    #[tokio::test]
    async fn acquire_grants_when_key_was_set() {
        let mut client = MockRedisClient::new();
        client.expect_set_nx_px().returning(|_, _, _| Ok(true));
        let lock = RedisLock::new(Arc::new(client), fixed_clock(100));

        let lease = lock.acquire("k", Duration::from_secs(30)).await.unwrap().unwrap();
        assert_eq!(lease.key, "k");
        // expires_at is derived from the injected clock (100s) + ttl (30s).
        assert_eq!(lease.expires_at, Utc.timestamp_opt(130, 0).unwrap());
    }

    #[tokio::test]
    async fn acquire_returns_none_when_held() {
        let mut client = MockRedisClient::new();
        client.expect_set_nx_px().returning(|_, _, _| Ok(false));
        let lock = RedisLock::new(Arc::new(client), fixed_clock(0));
        assert!(lock.acquire("k", Duration::from_secs(30)).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn key_is_namespaced() {
        let mut client = MockRedisClient::new();
        client
            .expect_set_nx_px()
            .withf(|key, _, _| key == "pi:lock:repo:org/api")
            .returning(|_, _, _| Ok(true));
        let lock = RedisLock::new(Arc::new(client), fixed_clock(0));
        assert!(lock.acquire("repo:org/api", Duration::from_secs(5)).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn refresh_extends_when_owner() {
        let mut client = MockRedisClient::new();
        client
            .expect_run_if_owner()
            .with(eq("pi:lock:k"), eq("tok"), eq(Op::Pexpire(30000)))
            .returning(|_, _, _| Ok(true));
        let lock = RedisLock::new(Arc::new(client), fixed_clock(200));
        let lease = Lease { key: "k".into(), token: "tok".into(), expires_at: Utc.timestamp_opt(0, 0).unwrap() };

        let renewed = lock.refresh(&lease, Duration::from_secs(30)).await.unwrap();
        assert_eq!(renewed.expires_at, Utc.timestamp_opt(230, 0).unwrap());
    }

    #[tokio::test]
    async fn refresh_reports_lost_when_not_owner() {
        let mut client = MockRedisClient::new();
        client.expect_run_if_owner().returning(|_, _, _| Ok(false));
        let lock = RedisLock::new(Arc::new(client), fixed_clock(0));
        let lease = Lease { key: "k".into(), token: "tok".into(), expires_at: Utc.timestamp_opt(0, 0).unwrap() };
        assert!(matches!(lock.refresh(&lease, Duration::from_secs(5)).await, Err(LockError::Lost(_))));
    }

    #[tokio::test]
    async fn release_deletes_with_token_check() {
        let mut client = MockRedisClient::new();
        client
            .expect_run_if_owner()
            .with(eq("pi:lock:k"), eq("tok"), eq(Op::Del))
            .returning(|_, _, _| Ok(true));
        let lock = RedisLock::new(Arc::new(client), fixed_clock(0));
        let lease = Lease { key: "k".into(), token: "tok".into(), expires_at: Utc.timestamp_opt(0, 0).unwrap() };
        assert!(lock.release(&lease).await.is_ok());
    }
}
