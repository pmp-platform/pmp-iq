//! Dual-engine persistence for LLM usage rows and spend budgets (M39).

use super::model::{
    Budget, BudgetInput, BudgetPeriod, BudgetScope, CostDimension, GroupModelTokens, LlmUsageInput,
    ModelTokens,
};
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, SqlitePool};
use uuid::Uuid;

/// Append + aggregate recorded LLM usage.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait LlmUsageRepository: Send + Sync {
    /// Append one usage row.
    async fn record(&self, usage: &LlmUsageInput) -> RepoResult<()>;
    /// Per-model token totals for a scope since `since` (powers budgets + totals).
    async fn usage_since(
        &self,
        scope: BudgetScope,
        scope_id: Option<Uuid>,
        since: DateTime<Utc>,
    ) -> RepoResult<Vec<ModelTokens>>;
    /// Per-(key, model) token totals grouped by a dimension since `since`.
    async fn grouped(
        &self,
        dimension: CostDimension,
        since: DateTime<Utc>,
    ) -> RepoResult<Vec<GroupModelTokens>>;
    /// Per-model token totals for one job execution (per-execution cost).
    async fn usage_for_execution(&self, execution_id: Uuid) -> RepoResult<Vec<ModelTokens>>;
}

/// CRUD for spend budgets.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait LlmBudgetRepository: Send + Sync {
    async fn create(&self, input: &BudgetInput) -> RepoResult<Budget>;
    async fn list(&self) -> RepoResult<Vec<Budget>>;
    async fn delete(&self, id: Uuid) -> RepoResult<()>;
}

/// `WHERE`/join fragments selecting usage for a budget scope. `$2` binds the
/// scope id (omitted for global). Job scope joins executions to reach `job_id`.
fn scope_sql(scope: BudgetScope) -> (&'static str, &'static str) {
    match scope {
        BudgetScope::Global => ("", ""),
        BudgetScope::Profile => ("", " AND u.ai_profile_id = $2"),
        BudgetScope::Application => ("", " AND u.application_id = $2"),
        BudgetScope::Job => (
            " JOIN job_executions e ON e.id = u.job_execution_id",
            " AND e.job_id = $2",
        ),
    }
}

/// The grouping key SQL expression, optional join, and whether the key is a
/// UUID column (decoded natively) vs. text (job type).
fn dimension_sql(dimension: CostDimension) -> (&'static str, &'static str, bool) {
    match dimension {
        CostDimension::Application => ("u.application_id", "", true),
        CostDimension::Profile => ("u.ai_profile_id", "", true),
        CostDimension::JobType => (
            "j.job_type",
            " JOIN job_executions e ON e.id = u.job_execution_id JOIN jobs j ON j.id = e.job_id",
            false,
        ),
    }
}

/// `SUM` of a BIGINT column cast back to BIGINT — Postgres `SUM(bigint)` returns
/// `numeric`, which won't decode into `i64` without this cast.
const SUM_IN: &str = "CAST(COALESCE(SUM(u.input_tokens),0) AS BIGINT)";
const SUM_OUT: &str = "CAST(COALESCE(SUM(u.output_tokens),0) AS BIGINT)";

macro_rules! usage_impl {
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
        impl LlmUsageRepository for $name {
            async fn record(&self, usage: &LlmUsageInput) -> RepoResult<()> {
                // Bind `occurred_at` explicitly rather than leaning on the column
                // default: SQLite's `CURRENT_TIMESTAMP` stores a space-separated,
                // offset-less string ("2026-07-01 00:45:28") that lexicographically
                // sorts *before* the RFC3339 value ("2026-07-01T00:00:00+00:00")
                // sqlx binds for the `occurred_at >= $since` filters, so a just-
                // written row would be wrongly excluded from same-period queries.
                // Binding the value here makes the write use sqlx's own encoding,
                // identical to the read side, on both engines.
                sqlx::query(&$xform(
                    "INSERT INTO llm_usage \
                       (id, job_execution_id, application_id, ai_profile_id, model, input_tokens, output_tokens, occurred_at) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
                ))
                .bind(Uuid::new_v4())
                .bind(usage.job_execution_id)
                .bind(usage.application_id)
                .bind(usage.ai_profile_id)
                .bind(&usage.model)
                .bind(usage.input_tokens)
                .bind(usage.output_tokens)
                .bind(Utc::now())
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn usage_since(
                &self,
                scope: BudgetScope,
                scope_id: Option<Uuid>,
                since: DateTime<Utc>,
            ) -> RepoResult<Vec<ModelTokens>> {
                let (join, filter) = scope_sql(scope);
                let sql = $xform(&format!(
                    "SELECT u.model, {SUM_IN}, {SUM_OUT} \
                     FROM llm_usage u{join} WHERE u.occurred_at >= $1{filter} GROUP BY u.model"
                ));
                let mut q = sqlx::query_as(&sql).bind(since);
                if !matches!(scope, BudgetScope::Global) {
                    q = q.bind(scope_id);
                }
                let rows: Vec<(String, i64, i64)> = q.fetch_all(&self.pool).await?;
                Ok(rows
                    .into_iter()
                    .map(|(model, input_tokens, output_tokens)| ModelTokens { model, input_tokens, output_tokens })
                    .collect())
            }

            async fn grouped(
                &self,
                dimension: CostDimension,
                since: DateTime<Utc>,
            ) -> RepoResult<Vec<GroupModelTokens>> {
                let (key_expr, join, uuid_key) = dimension_sql(dimension);
                let sql = $xform(&format!(
                    "SELECT {key_expr}, u.model, {SUM_IN}, {SUM_OUT} \
                     FROM llm_usage u{join} WHERE u.occurred_at >= $1 AND {key_expr} IS NOT NULL \
                     GROUP BY {key_expr}, u.model"
                ));
                if uuid_key {
                    let rows: Vec<(Uuid, String, i64, i64)> =
                        sqlx::query_as(&sql).bind(since).fetch_all(&self.pool).await?;
                    Ok(rows
                        .into_iter()
                        .map(|(id, model, i, o)| GroupModelTokens {
                            key: id.to_string(),
                            model,
                            input_tokens: i,
                            output_tokens: o,
                        })
                        .collect())
                } else {
                    let rows: Vec<(String, String, i64, i64)> =
                        sqlx::query_as(&sql).bind(since).fetch_all(&self.pool).await?;
                    Ok(rows
                        .into_iter()
                        .map(|(key, model, i, o)| GroupModelTokens {
                            key,
                            model,
                            input_tokens: i,
                            output_tokens: o,
                        })
                        .collect())
                }
            }

            async fn usage_for_execution(&self, execution_id: Uuid) -> RepoResult<Vec<ModelTokens>> {
                let rows: Vec<(String, i64, i64)> = sqlx::query_as(&$xform(&format!(
                    "SELECT u.model, {SUM_IN}, {SUM_OUT} FROM llm_usage u \
                     WHERE u.job_execution_id = $1 GROUP BY u.model"
                )))
                .bind(execution_id)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows
                    .into_iter()
                    .map(|(model, input_tokens, output_tokens)| ModelTokens { model, input_tokens, output_tokens })
                    .collect())
            }
        }
    };
}

usage_impl!(PgLlmUsageRepository, PgPool, identity);
usage_impl!(SqliteLlmUsageRepository, SqlitePool, to_sqlite);

#[derive(sqlx::FromRow)]
struct BudgetRow {
    id: Uuid,
    scope: String,
    scope_id: Option<Uuid>,
    period: String,
    limit_usd: f64,
    hard_stop: bool,
}

impl TryFrom<BudgetRow> for Budget {
    type Error = String;
    fn try_from(r: BudgetRow) -> Result<Self, String> {
        Ok(Budget {
            id: r.id,
            scope: BudgetScope::parse(&r.scope)?,
            scope_id: r.scope_id,
            period: BudgetPeriod::parse(&r.period)?,
            limit_usd: r.limit_usd,
            hard_stop: r.hard_stop,
        })
    }
}

macro_rules! budget_impl {
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
        impl LlmBudgetRepository for $name {
            async fn create(&self, input: &BudgetInput) -> RepoResult<Budget> {
                let id = Uuid::new_v4();
                sqlx::query(&$xform(
                    "INSERT INTO llm_budgets (id, scope, scope_id, period, limit_usd, hard_stop) \
                     VALUES ($1,$2,$3,$4,$5,$6)",
                ))
                .bind(id)
                .bind(input.scope.as_str())
                .bind(input.scope_id)
                .bind(input.period.as_str())
                .bind(input.limit_usd)
                .bind(input.hard_stop)
                .execute(&self.pool)
                .await?;
                Ok(Budget {
                    id,
                    scope: input.scope,
                    scope_id: input.scope_id,
                    period: input.period,
                    limit_usd: input.limit_usd,
                    hard_stop: input.hard_stop,
                })
            }

            async fn list(&self) -> RepoResult<Vec<Budget>> {
                let rows: Vec<BudgetRow> = sqlx::query_as(
                    "SELECT id, scope, scope_id, period, limit_usd, hard_stop FROM llm_budgets ORDER BY scope, period",
                )
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().filter_map(|r| Budget::try_from(r).ok()).collect())
            }

            async fn delete(&self, id: Uuid) -> RepoResult<()> {
                sqlx::query(&$xform("DELETE FROM llm_budgets WHERE id = $1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }
        }
    };
}

budget_impl!(PgLlmBudgetRepository, PgPool, identity);
budget_impl!(SqliteLlmBudgetRepository, SqlitePool, to_sqlite);
