//! DORA event + summary model (M47).

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// A captured deployment event.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Deployment {
    pub id: Uuid,
    pub application_id: Option<Uuid>,
    pub environment: String,
    pub sha: Option<String>,
    pub succeeded: bool,
    pub deployed_at: DateTime<Utc>,
    pub first_commit_at: Option<DateTime<Utc>>,
}

/// Fields to record a deployment.
#[derive(Debug, Clone)]
pub struct NewDeployment {
    pub application_id: Uuid,
    pub environment: String,
    pub sha: Option<String>,
    pub succeeded: bool,
    pub first_commit_at: Option<DateTime<Utc>>,
}

/// A captured incident (open/resolve markers for MTTR).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Incident {
    pub id: Uuid,
    pub application_id: Option<Uuid>,
    pub caused_by: Option<Uuid>,
    pub opened_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

/// The computed DORA summary over a window (`None` where there is no evidence).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DoraSummary {
    /// Successful deployments per week over the window.
    pub deploy_frequency_weekly: f64,
    /// Median lead time (hours) from first commit to deploy.
    pub lead_time_hours: Option<f64>,
    /// Fraction of deployments that caused an incident (0..1).
    pub change_failure_rate: f64,
    /// Median time-to-restore (hours) for resolved incidents.
    pub mttr_hours: Option<f64>,
    /// Overall performance tier: elite | high | medium | low.
    pub tier: String,
    /// Deployments considered in the window.
    pub deployments: usize,
    /// Incidents considered in the window.
    pub incidents: usize,
}
