//! Dual-engine persistence for application quality metrics.

use super::model::{ApplicationMetric, Metric};
use super::registry::category_for;
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, SqlitePool};
use uuid::Uuid;

/// Record + read application metrics.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ApplicationMetricsRepository: Send + Sync {
    /// Append a collected metric set for an application (keeps history).
    async fn record(&self, application_id: Uuid, source: &str, metrics: &[Metric]) -> RepoResult<()>;
    /// The latest value of each metric for one application.
    async fn latest_for_application(&self, application_id: Uuid) -> RepoResult<Vec<ApplicationMetric>>;
    /// The latest value of each metric across all applications (for the dashboard).
    async fn latest_all(&self) -> RepoResult<Vec<ApplicationMetric>>;
    /// Timestamped readings of a metric across the fleet since `since` (trends).
    async fn history(&self, metric_key: &str, since: DateTime<Utc>) -> RepoResult<Vec<super::series::Point>>;
    /// Fleet readings of a metric grouped by an (allowlisted) application column.
    async fn history_by_dimension(
        &self,
        metric_key: &str,
        dimension: &str,
        since: DateTime<Utc>,
    ) -> RepoResult<Vec<(String, super::series::Point)>>;
    /// Timestamped readings of a metric for one application (sparklines).
    async fn app_history(
        &self,
        application_id: Uuid,
        metric_key: &str,
        since: DateTime<Utc>,
    ) -> RepoResult<Vec<super::series::Point>>;
}

#[derive(FromRow)]
struct Row {
    id: Uuid,
    application_id: Uuid,
    metric_key: String,
    value: f64,
    unit: Option<String>,
    source: String,
    category: String,
    collected_at: DateTime<Utc>,
}

impl From<Row> for ApplicationMetric {
    fn from(r: Row) -> Self {
        ApplicationMetric {
            id: r.id,
            application_id: r.application_id,
            metric_key: r.metric_key,
            value: r.value,
            unit: r.unit,
            source: r.source,
            category: r.category,
            collected_at: r.collected_at,
        }
    }
}

const COLS: &str = "id, application_id, metric_key, value, unit, source, category, collected_at";
/// Each row whose `collected_at` is the most recent for its (app, key).
const LATEST_WHERE: &str = "collected_at = (SELECT MAX(collected_at) FROM application_metrics x \
     WHERE x.application_id = m.application_id AND x.metric_key = m.metric_key)";

macro_rules! metrics_impl {
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
        impl ApplicationMetricsRepository for $name {
            async fn record(
                &self,
                application_id: Uuid,
                source: &str,
                metrics: &[Metric],
            ) -> RepoResult<()> {
                for m in metrics {
                    // Bind `collected_at` explicitly so the write uses sqlx's own
                    // timestamp encoding, identical to the `collected_at >= $since`
                    // history/series filters. Leaning on SQLite's CURRENT_TIMESTAMP
                    // default stores a space-separated, offset-less string that
                    // sorts before sqlx's RFC3339 bound value, wrongly excluding
                    // same-period rows at period boundaries.
                    sqlx::query(&$xform(
                        "INSERT INTO application_metrics \
                           (id, application_id, metric_key, value, unit, source, category, collected_at) \
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
                    ))
                    .bind(Uuid::new_v4())
                    .bind(application_id)
                    .bind(&m.key)
                    .bind(m.value)
                    .bind(m.unit.as_deref())
                    .bind(source)
                    .bind(category_for(&m.key).as_str())
                    .bind(Utc::now())
                    .execute(&self.pool)
                    .await?;
                }
                Ok(())
            }

            async fn latest_for_application(
                &self,
                application_id: Uuid,
            ) -> RepoResult<Vec<ApplicationMetric>> {
                let rows: Vec<Row> = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM application_metrics m \
                     WHERE m.application_id = $1 AND {LATEST_WHERE} ORDER BY metric_key"
                )))
                .bind(application_id)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(ApplicationMetric::from).collect())
            }

            async fn latest_all(&self) -> RepoResult<Vec<ApplicationMetric>> {
                let rows: Vec<Row> = sqlx::query_as(&format!(
                    "SELECT {COLS} FROM application_metrics m WHERE {LATEST_WHERE}"
                ))
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(ApplicationMetric::from).collect())
            }

            async fn history(&self, metric_key: &str, since: DateTime<Utc>) -> RepoResult<Vec<super::series::Point>> {
                let rows: Vec<(DateTime<Utc>, f64)> = sqlx::query_as(&$xform(
                    "SELECT collected_at, value FROM application_metrics \
                     WHERE metric_key=$1 AND collected_at >= $2 ORDER BY collected_at",
                ))
                .bind(metric_key)
                .bind(since)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(|(at, value)| super::series::Point { at, value }).collect())
            }

            async fn history_by_dimension(
                &self,
                metric_key: &str,
                dimension: &str,
                since: DateTime<Utc>,
            ) -> RepoResult<Vec<(String, super::series::Point)>> {
                // The caller must pass an allowlisted column (see series::allowed_dimension).
                if !super::series::allowed_dimension(dimension) {
                    return Ok(vec![]);
                }
                let sql = $xform(&format!(
                    "SELECT COALESCE(a.{dimension}, 'unknown'), m.collected_at, m.value \
                     FROM application_metrics m JOIN applications a ON a.id=m.application_id \
                     WHERE m.metric_key=$1 AND m.collected_at >= $2 ORDER BY m.collected_at"
                ));
                let rows: Vec<(String, DateTime<Utc>, f64)> =
                    sqlx::query_as(&sql).bind(metric_key).bind(since).fetch_all(&self.pool).await?;
                Ok(rows.into_iter().map(|(dim, at, value)| (dim, super::series::Point { at, value })).collect())
            }

            async fn app_history(
                &self,
                application_id: Uuid,
                metric_key: &str,
                since: DateTime<Utc>,
            ) -> RepoResult<Vec<super::series::Point>> {
                let rows: Vec<(DateTime<Utc>, f64)> = sqlx::query_as(&$xform(
                    "SELECT collected_at, value FROM application_metrics \
                     WHERE application_id=$1 AND metric_key=$2 AND collected_at >= $3 ORDER BY collected_at",
                ))
                .bind(application_id)
                .bind(metric_key)
                .bind(since)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(|(at, value)| super::series::Point { at, value }).collect())
            }
        }
    };
}

metrics_impl!(PgApplicationMetricsRepository, PgPool, identity);
metrics_impl!(SqliteApplicationMetricsRepository, SqlitePool, to_sqlite);
