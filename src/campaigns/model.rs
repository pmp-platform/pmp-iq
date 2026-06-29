//! Model for batch-change campaigns (M30).

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// A named change applied across many repositories, backed by one multi-repo
/// agent task (whose targets carry the per-repo branch/PR/status).
#[derive(Debug, Clone, Serialize)]
pub struct Campaign {
    pub id: Uuid,
    pub name: String,
    pub instruction: String,
    /// JSON describing how the repository set was selected (for audit).
    pub selection: String,
    pub task_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// Fields to create a campaign.
#[derive(Debug, Clone)]
pub struct NewCampaign {
    pub name: String,
    pub instruction: String,
    pub selection: String,
    pub task_id: Uuid,
}
