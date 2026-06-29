//! Process-local in-memory distributed lock (the simple first backend).

use super::{DistributedLock, Lease, LockError, LockResult, chrono_ttl};
use crate::jobs::clock::Clock;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

/// One held key: who holds it and until when.
struct Entry {
    token: String,
    expires_at: DateTime<Utc>,
}

/// A TTL lock backed by an in-process map. Correct within a single instance;
/// horizontal scaling needs the SQL-backed backend.
pub struct InMemoryLock {
    clock: Arc<dyn Clock>,
    entries: Mutex<HashMap<String, Entry>>,
}

impl InMemoryLock {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn new_token() -> String {
        Uuid::new_v4().to_string()
    }
}

#[async_trait]
impl DistributedLock for InMemoryLock {
    async fn acquire(&self, key: &str, ttl: Duration) -> LockResult<Option<Lease>> {
        let now = self.clock.now();
        let expires_at = now + chrono_ttl(ttl);
        let mut entries = self.entries.lock().expect("lock map poisoned");
        if let Some(entry) = entries.get(key) {
            if entry.expires_at > now {
                return Ok(None);
            }
        }
        let token = Self::new_token();
        entries.insert(key.to_string(), Entry { token: token.clone(), expires_at });
        Ok(Some(Lease { key: key.to_string(), token, expires_at }))
    }

    async fn refresh(&self, lease: &Lease, ttl: Duration) -> LockResult<Lease> {
        let now = self.clock.now();
        let expires_at = now + chrono_ttl(ttl);
        let mut entries = self.entries.lock().expect("lock map poisoned");
        match entries.get(&lease.key) {
            Some(entry) if entry.token == lease.token => {
                entries.insert(
                    lease.key.clone(),
                    Entry { token: lease.token.clone(), expires_at },
                );
                Ok(Lease { key: lease.key.clone(), token: lease.token.clone(), expires_at })
            }
            _ => Err(LockError::Lost(lease.key.clone())),
        }
    }

    async fn release(&self, lease: &Lease) -> LockResult<()> {
        let mut entries = self.entries.lock().expect("lock map poisoned");
        if let Some(entry) = entries.get(&lease.key) {
            if entry.token == lease.token {
                entries.remove(&lease.key);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::clock::MockClock;
    use chrono::TimeZone;
    use std::sync::Mutex as StdMutex;

    /// A clock whose "now" can be advanced for deterministic expiry tests.
    struct SettableClock(StdMutex<DateTime<Utc>>);

    impl SettableClock {
        fn new(secs: i64) -> Arc<Self> {
            Arc::new(Self(StdMutex::new(Utc.timestamp_opt(secs, 0).unwrap())))
        }
        fn advance(&self, secs: i64) {
            let mut g = self.0.lock().unwrap();
            *g += chrono::Duration::seconds(secs);
        }
    }

    impl Clock for SettableClock {
        fn now(&self) -> DateTime<Utc> {
            *self.0.lock().unwrap()
        }
    }

    fn fixed_clock(secs: i64) -> Arc<MockClock> {
        let mut clock = MockClock::new();
        clock.expect_now().returning(move || Utc.timestamp_opt(secs, 0).unwrap());
        Arc::new(clock)
    }

    #[tokio::test]
    async fn second_holder_blocked_until_expiry() {
        let clock = SettableClock::new(100);
        let lock = InMemoryLock::new(clock.clone());

        let a = lock.acquire("k", Duration::from_secs(30)).await.unwrap();
        assert!(a.is_some(), "first holder acquires");
        // A second acquisition while the lease is valid is refused.
        assert!(lock.acquire("k", Duration::from_secs(30)).await.unwrap().is_none());

        // After expiry the key is reclaimable.
        clock.advance(31);
        assert!(lock.acquire("k", Duration::from_secs(30)).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn refresh_extends_and_release_frees() {
        let clock = SettableClock::new(100);
        let lock = InMemoryLock::new(clock.clone());
        let lease = lock.acquire("k", Duration::from_secs(30)).await.unwrap().unwrap();

        // Just before expiry, a refresh pushes the deadline out.
        clock.advance(20);
        let renewed = lock.refresh(&lease, Duration::from_secs(30)).await.unwrap();
        clock.advance(20); // 40s since acquire, but only 20s since refresh
        assert!(lock.acquire("k", Duration::from_secs(30)).await.unwrap().is_none());

        // Releasing frees it immediately.
        lock.release(&renewed).await.unwrap();
        assert!(lock.acquire("k", Duration::from_secs(30)).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn refresh_after_takeover_reports_lost() {
        let clock = fixed_clock(0);
        let lock = InMemoryLock::new(clock);
        let lease = Lease { key: "k".into(), token: "stale".into(), expires_at: Utc.timestamp_opt(0, 0).unwrap() };
        // No entry exists for this token.
        assert!(matches!(lock.refresh(&lease, Duration::from_secs(5)).await, Err(LockError::Lost(_))));
    }
}
