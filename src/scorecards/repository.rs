//! Dual-engine persistence for scorecard checks + results (M43).

use super::engine::{Check, CheckResult};
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, SqlitePool};
use uuid::Uuid;

/// Check definitions + per-application results.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ScorecardRepository: Send + Sync {
    async fn list_checks(&self) -> RepoResult<Vec<Check>>;
    async fn upsert_check(&self, check: &Check) -> RepoResult<()>;
    async fn delete_check(&self, id: &str) -> RepoResult<()>;
    async fn count_checks(&self) -> RepoResult<i64>;
    /// Replace an application's latest results with a fresh evaluation.
    async fn record(&self, application_id: Uuid, results: &[CheckResult]) -> RepoResult<()>;
}

#[derive(sqlx::FromRow)]
struct CheckRow {
    id: String,
    description: String,
    rule: String,
    params: Value,
    weight: i32,
    severity: String,
    enabled: bool,
}

impl From<CheckRow> for Check {
    fn from(r: CheckRow) -> Self {
        Check {
            id: r.id,
            description: r.description,
            rule: r.rule,
            params: r.params,
            weight: r.weight,
            severity: r.severity,
            enabled: r.enabled,
        }
    }
}

macro_rules! scorecard_impl {
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
        impl ScorecardRepository for $name {
            async fn list_checks(&self) -> RepoResult<Vec<Check>> {
                let rows: Vec<CheckRow> = sqlx::query_as(
                    "SELECT id, description, rule, params, weight, severity, enabled FROM scorecard_checks ORDER BY id",
                )
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(Check::from).collect())
            }

            async fn upsert_check(&self, check: &Check) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "INSERT INTO scorecard_checks (id, description, rule, params, weight, severity, enabled) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7) \
                     ON CONFLICT (id) DO UPDATE SET description=excluded.description, rule=excluded.rule, \
                     params=excluded.params, weight=excluded.weight, severity=excluded.severity, enabled=excluded.enabled",
                ))
                .bind(&check.id)
                .bind(&check.description)
                .bind(&check.rule)
                .bind(&check.params)
                .bind(check.weight)
                .bind(&check.severity)
                .bind(check.enabled)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn delete_check(&self, id: &str) -> RepoResult<()> {
                sqlx::query(&$xform("DELETE FROM scorecard_checks WHERE id=$1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }

            async fn count_checks(&self) -> RepoResult<i64> {
                let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM scorecard_checks").fetch_one(&self.pool).await?;
                Ok(n)
            }

            async fn record(&self, application_id: Uuid, results: &[CheckResult]) -> RepoResult<()> {
                sqlx::query(&$xform("DELETE FROM scorecard_results WHERE application_id=$1"))
                    .bind(application_id)
                    .execute(&self.pool)
                    .await?;
                for r in results {
                    sqlx::query(&$xform(
                        "INSERT INTO scorecard_results (id, application_id, check_id, passed, detail) \
                         VALUES ($1,$2,$3,$4,$5)",
                    ))
                    .bind(Uuid::new_v4())
                    .bind(application_id)
                    .bind(&r.check_id)
                    .bind(r.passed)
                    .bind(&r.detail)
                    .execute(&self.pool)
                    .await?;
                }
                Ok(())
            }
        }
    };
}

scorecard_impl!(PgScorecardRepository, PgPool, identity);
scorecard_impl!(SqliteScorecardRepository, SqlitePool, to_sqlite);
