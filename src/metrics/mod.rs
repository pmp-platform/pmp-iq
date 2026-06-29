//! Per-application quality metrics (M31): an LLM collection job that extracts
//! tests / coverage / complexity / LOC from a repository's CI + codebase, and a
//! dual-engine store with history (feeding the dashboard, M32).

pub mod job;
pub mod model;
pub mod repository;

pub use job::{CollectMetricsDeps, CollectMetricsJob, JOB_TYPE, ensure_job};
pub use model::{ApplicationMetric, Metric};
pub use repository::{
    ApplicationMetricsRepository, PgApplicationMetricsRepository, SqliteApplicationMetricsRepository,
};
