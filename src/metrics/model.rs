//! Model for per-application quality metrics (M31).

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// A single normalised metric value to record.
#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    pub key: String,
    pub value: f64,
    pub unit: Option<String>,
}

impl Metric {
    pub fn new(key: impl Into<String>, value: f64, unit: Option<&str>) -> Self {
        Self { key: key.into(), value, unit: unit.map(str::to_string) }
    }
}

/// A persisted metric row.
#[derive(Debug, Clone, Serialize)]
pub struct ApplicationMetric {
    pub id: Uuid,
    pub application_id: Uuid,
    pub metric_key: String,
    pub value: f64,
    pub unit: Option<String>,
    pub source: String,
    /// Theme the metric belongs to (M33), stamped from the registry at write time.
    pub category: String,
    pub collected_at: DateTime<Utc>,
}
