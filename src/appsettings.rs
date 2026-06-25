//! Generic key/value application settings persisted in `app_settings`.

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, SqlitePool};

/// Read/write access to persisted key/value settings.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait SettingsRepository: Send + Sync {
    /// Fetch a setting value by key, or `None` if it is absent.
    async fn get(&self, key: &str) -> RepoResult<Option<Value>>;
    /// Insert or update a setting value.
    async fn set(&self, key: &str, value: &Value) -> RepoResult<()>;
}

/// Generate a Postgres and a SQLite implementation from one body. The SQL is
/// authored in Postgres `$N` style and translated for SQLite.
macro_rules! settings_repo_impl {
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
        impl SettingsRepository for $name {
            async fn get(&self, key: &str) -> RepoResult<Option<Value>> {
                let row: Option<(Value,)> =
                    sqlx::query_as(&$xform("SELECT value FROM app_settings WHERE key = $1"))
                        .bind(key)
                        .fetch_optional(&self.pool)
                        .await?;
                Ok(row.map(|(v,)| v))
            }

            async fn set(&self, key: &str, value: &Value) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "INSERT INTO app_settings (key, value, updated_at) \
                     VALUES ($1, $2, CURRENT_TIMESTAMP) \
                     ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, \
                     updated_at = CURRENT_TIMESTAMP",
                ))
                .bind(key)
                .bind(value)
                .execute(&self.pool)
                .await?;
                Ok(())
            }
        }
    };
}

settings_repo_impl!(PgSettingsRepository, PgPool, identity);
settings_repo_impl!(SqliteSettingsRepository, SqlitePool, to_sqlite);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn mock_round_trips_a_value() {
        let mut mock = MockSettingsRepository::new();
        mock.expect_set().returning(|_, _| Ok(()));
        mock.expect_get()
            .returning(|_| Ok(Some(json!({"theme": "dark"}))));

        mock.set("ui", &json!({"theme": "dark"})).await.unwrap();
        let got = mock.get("ui").await.unwrap();
        assert_eq!(got, Some(json!({"theme": "dark"})));
    }
}
