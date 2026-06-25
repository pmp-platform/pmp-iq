//! A minimal, idempotent migration applier.
//!
//! Migrations are embedded at compile time. SQLite (the zero-config default) is
//! migrated automatically at boot; PostgreSQL deployments typically use dbmate
//! but the same applier is used by the test harness. Applied versions are
//! tracked in `_app_migrations` so re-running is a no-op.

use super::{Database, RepoResult, to_sqlite};
use sqlx::Executor;
use std::collections::HashSet;

type Migrations = &'static [(&'static str, &'static str)];

/// Embedded PostgreSQL migrations (dbmate format).
pub const PG_MIGRATIONS: Migrations = &[
    ("001_app_settings", include_str!("../../db/migrations/20260101000001_create_app_settings.sql")),
    ("002_repository_accounts", include_str!("../../db/migrations/20260101000002_create_repository_accounts.sql")),
    ("003_ai_agent_profiles", include_str!("../../db/migrations/20260101000003_create_ai_agent_profiles.sql")),
    ("004_jobs", include_str!("../../db/migrations/20260101000004_create_jobs.sql")),
    ("005_repositories", include_str!("../../db/migrations/20260101000005_create_repositories.sql")),
    ("006_platform_model", include_str!("../../db/migrations/20260101000006_create_platform_model.sql")),
    ("007_jobs_pause_and_locks", include_str!("../../db/migrations/20260101000007_jobs_pause_and_locks.sql")),
];

/// Embedded SQLite migrations (dbmate format).
pub const SQLITE_MIGRATIONS: Migrations = &[
    ("001_app_settings", include_str!("../../db/migrations_sqlite/20260101000001_create_app_settings.sql")),
    ("002_repository_accounts", include_str!("../../db/migrations_sqlite/20260101000002_create_repository_accounts.sql")),
    ("003_ai_agent_profiles", include_str!("../../db/migrations_sqlite/20260101000003_create_ai_agent_profiles.sql")),
    ("004_jobs", include_str!("../../db/migrations_sqlite/20260101000004_create_jobs.sql")),
    ("005_repositories", include_str!("../../db/migrations_sqlite/20260101000005_create_repositories.sql")),
    ("006_platform_model", include_str!("../../db/migrations_sqlite/20260101000006_create_platform_model.sql")),
    ("007_jobs_pause_and_locks", include_str!("../../db/migrations_sqlite/20260101000007_jobs_pause_and_locks.sql")),
];

/// Migrations matching the database's engine.
pub fn for_engine(db: &Database) -> Migrations {
    match db {
        Database::Postgres(_) => PG_MIGRATIONS,
        Database::Sqlite(_) => SQLITE_MIGRATIONS,
    }
}

/// Apply any unapplied migrations to the database.
pub async fn apply(db: &Database, migrations: Migrations) -> RepoResult<()> {
    ensure_table(db).await?;
    let applied = applied_versions(db).await?;
    for (version, content) in migrations {
        if applied.contains(*version) {
            continue;
        }
        let up = extract_up(content);
        if !up.trim().is_empty() {
            run_raw(db, &up).await?;
        }
        record(db, version).await?;
    }
    Ok(())
}

async fn ensure_table(db: &Database) -> RepoResult<()> {
    run_raw(db, "CREATE TABLE IF NOT EXISTS _app_migrations (version TEXT PRIMARY KEY)").await
}

async fn applied_versions(db: &Database) -> RepoResult<HashSet<String>> {
    let rows: Vec<(String,)> = match db {
        Database::Postgres(pool) => {
            sqlx::query_as("SELECT version FROM _app_migrations").fetch_all(pool).await?
        }
        Database::Sqlite(pool) => {
            sqlx::query_as("SELECT version FROM _app_migrations").fetch_all(pool).await?
        }
    };
    Ok(rows.into_iter().map(|(v,)| v).collect())
}

async fn record(db: &Database, version: &str) -> RepoResult<()> {
    match db {
        Database::Postgres(pool) => {
            sqlx::query("INSERT INTO _app_migrations (version) VALUES ($1)")
                .bind(version)
                .execute(pool)
                .await?;
        }
        Database::Sqlite(pool) => {
            sqlx::query(&to_sqlite("INSERT INTO _app_migrations (version) VALUES ($1)"))
                .bind(version)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

async fn run_raw(db: &Database, sql: &str) -> RepoResult<()> {
    match db {
        Database::Postgres(pool) => {
            pool.execute(sqlx::raw_sql(sql)).await?;
        }
        Database::Sqlite(pool) => {
            pool.execute(sqlx::raw_sql(sql)).await?;
        }
    }
    Ok(())
}

/// Extract the SQL between `-- migrate:up` and `-- migrate:down`.
pub fn extract_up(sql: &str) -> String {
    let after_up = sql.split_once("-- migrate:up").map(|(_, r)| r).unwrap_or(sql);
    after_up
        .split_once("-- migrate:down")
        .map(|(up, _)| up)
        .unwrap_or(after_up)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_up_pulls_up_block_only() {
        let sql = "-- migrate:up\nCREATE TABLE t(id int);\n-- migrate:down\nDROP TABLE t;";
        let up = extract_up(sql);
        assert!(up.contains("CREATE TABLE t"));
        assert!(!up.contains("DROP TABLE"));
    }
}
