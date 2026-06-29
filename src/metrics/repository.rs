//! Dual-engine persistence for application quality metrics.

use super::model::{ApplicationMetric, Metric};
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
}

#[derive(FromRow)]
struct Row {
    id: Uuid,
    application_id: Uuid,
    metric_key: String,
    value: f64,
    unit: Option<String>,
    source: String,
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
            collected_at: r.collected_at,
        }
    }
}

const COLS: &str = "id, application_id, metric_key, value, unit, source, collected_at";
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
                    sqlx::query(&$xform(
                        "INSERT INTO application_metrics \
                           (id, application_id, metric_key, value, unit, source) \
                         VALUES ($1,$2,$3,$4,$5,$6)",
                    ))
                    .bind(Uuid::new_v4())
                    .bind(application_id)
                    .bind(&m.key)
                    .bind(m.value)
                    .bind(m.unit.as_deref())
                    .bind(source)
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
        }
    };
}

metrics_impl!(PgApplicationMetricsRepository, PgPool, identity);
metrics_impl!(SqliteApplicationMetricsRepository, SqlitePool, to_sqlite);
