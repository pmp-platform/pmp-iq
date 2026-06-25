//! Database connection layer.
//!
//! Two engines are supported: SQLite (the zero-config default) and PostgreSQL.
//! [`Database`] is an enum over the two connection pools. Feature-level data
//! access goes through repository traits, each with a Postgres and a SQLite
//! implementation (see [`crate::store`]), so the engine is an implementation
//! detail that can be mocked in unit tests.

use crate::config::{DatabaseConfig, DbEngine};
use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{PgPool, SqlitePool};

pub mod error;
pub mod migrate;

pub use error::{RepoError, RepoResult};

/// A connected database handle over one of the supported engines.
#[derive(Clone)]
pub enum Database {
    Postgres(PgPool),
    Sqlite(SqlitePool),
}

impl Database {
    /// Connect using the configured engine (inferred from the URL scheme).
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, RepoError> {
        match config.engine() {
            DbEngine::Postgres => {
                let pool = PgPoolOptions::new()
                    .max_connections(config.max_connections)
                    .connect(&config.url)
                    .await?;
                Ok(Database::Postgres(pool))
            }
            DbEngine::Sqlite => {
                let pool = SqlitePoolOptions::new()
                    .max_connections(config.max_connections)
                    .connect(&config.url)
                    .await?;
                Ok(Database::Sqlite(pool))
            }
        }
    }

    pub fn from_pg(pool: PgPool) -> Self {
        Database::Postgres(pool)
    }

    pub fn from_sqlite(pool: SqlitePool) -> Self {
        Database::Sqlite(pool)
    }

    pub fn engine(&self) -> DbEngine {
        match self {
            Database::Postgres(_) => DbEngine::Postgres,
            Database::Sqlite(_) => DbEngine::Sqlite,
        }
    }

    /// Lightweight readiness probe used by the health endpoint.
    pub async fn ping(&self) -> Result<(), RepoError> {
        match self {
            Database::Postgres(pool) => {
                sqlx::query("SELECT 1").execute(pool).await?;
            }
            Database::Sqlite(pool) => {
                sqlx::query("SELECT 1").execute(pool).await?;
            }
        }
        Ok(())
    }
}

/// Identity SQL transform (Postgres uses `$N` placeholders as authored).
#[inline]
pub fn identity(sql: &str) -> String {
    sql.to_string()
}

/// Translate Postgres `$N` placeholders into SQLite `?N` placeholders.
///
/// Queries are authored once in Postgres style and translated for the SQLite
/// implementations. The project's SQL contains no `$N` sequences inside string
/// literals, so this character-level rewrite is safe.
pub fn to_sqlite(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek().map(|n| n.is_ascii_digit()).unwrap_or(false) {
            out.push('?');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_placeholders() {
        assert_eq!(to_sqlite("SELECT $1, $2 WHERE x=$10"), "SELECT ?1, ?2 WHERE x=?10");
    }

    #[test]
    fn leaves_non_placeholder_dollars() {
        assert_eq!(to_sqlite("a $ b"), "a $ b");
    }
}
