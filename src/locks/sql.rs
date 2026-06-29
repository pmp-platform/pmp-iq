//! SQL-backed distributed lock over the `controller_locks` table. Correct across
//! multiple instances (the same row arbitrates every holder).

use super::{DistributedLock, Lease, LockError, LockResult, chrono_ttl};
use crate::db::{identity, to_sqlite};
use crate::jobs::clock::Clock;
use async_trait::async_trait;
use sqlx::{PgPool, SqlitePool};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

fn backend(err: sqlx::Error) -> LockError {
    LockError::Backend(err.to_string())
}

macro_rules! sql_lock_impl {
    ($name:ident, $pool:ty, $xform:path) => {
        pub struct $name {
            pool: $pool,
            clock: Arc<dyn Clock>,
        }

        impl $name {
            pub fn new(pool: $pool, clock: Arc<dyn Clock>) -> Self {
                Self { pool, clock }
            }
        }

        #[async_trait]
        impl DistributedLock for $name {
            async fn acquire(&self, key: &str, ttl: Duration) -> LockResult<Option<Lease>> {
                let now = self.clock.now();
                let expires_at = now + chrono_ttl(ttl);
                let token = Uuid::new_v4().to_string();
                let res = sqlx::query(&$xform(
                    "INSERT INTO controller_locks (name, holder, expires_at) VALUES ($1,$2,$3) \
                     ON CONFLICT (name) DO UPDATE SET holder=$2, expires_at=$3 \
                     WHERE controller_locks.holder=$2 OR controller_locks.expires_at < $4",
                ))
                .bind(key)
                .bind(&token)
                .bind(expires_at)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(backend)?;
                if res.rows_affected() > 0 {
                    Ok(Some(Lease { key: key.to_string(), token, expires_at }))
                } else {
                    Ok(None)
                }
            }

            async fn refresh(&self, lease: &Lease, ttl: Duration) -> LockResult<Lease> {
                let expires_at = self.clock.now() + chrono_ttl(ttl);
                let res = sqlx::query(&$xform(
                    "INSERT INTO controller_locks (name, holder, expires_at) VALUES ($1,$2,$3) \
                     ON CONFLICT (name) DO UPDATE SET expires_at=$3 \
                     WHERE controller_locks.holder=$2",
                ))
                .bind(&lease.key)
                .bind(&lease.token)
                .bind(expires_at)
                .execute(&self.pool)
                .await
                .map_err(backend)?;
                if res.rows_affected() > 0 {
                    Ok(Lease { key: lease.key.clone(), token: lease.token.clone(), expires_at })
                } else {
                    Err(LockError::Lost(lease.key.clone()))
                }
            }

            async fn release(&self, lease: &Lease) -> LockResult<()> {
                sqlx::query(&$xform("DELETE FROM controller_locks WHERE name=$1 AND holder=$2"))
                    .bind(&lease.key)
                    .bind(&lease.token)
                    .execute(&self.pool)
                    .await
                    .map_err(backend)?;
                Ok(())
            }
        }
    };
}

sql_lock_impl!(PgSqlLock, PgPool, identity);
sql_lock_impl!(SqliteSqlLock, SqlitePool, to_sqlite);
