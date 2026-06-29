//! Model for AI Agent change tasks and their message transcripts.

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// Task lifecycle status, stored as a lowercase string.
pub mod status {
    pub const RUNNING: &str = "running";
    pub const AWAITING_INPUT: &str = "awaiting_input";
    pub const PR_OPEN: &str = "pr_open";
    pub const FAILED: &str = "failed";
}

/// Message roles.
pub mod role {
    pub const USER: &str = "user";
    pub const AGENT: &str = "agent";
}

/// An AI Agent change task: a session over one application's repository.
#[derive(Debug, Clone, Serialize)]
pub struct AgentTask {
    pub id: Uuid,
    pub application_id: Uuid,
    pub repository_id: Uuid,
    pub title: String,
    pub status: String,
    pub branch_name: String,
    pub pr_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One message in a task's transcript (a user instruction or an agent reply).
#[derive(Debug, Clone, Serialize)]
pub struct AgentTaskMessage {
    pub id: Uuid,
    pub task_id: Uuid,
    pub role: String,
    pub content: String,
    pub execution_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Fields to create a task (the branch name is derived from the new id).
#[derive(Debug, Clone)]
pub struct NewAgentTask {
    pub application_id: Uuid,
    pub repository_id: Uuid,
    pub title: String,
}

/// Fields to append a message to a task.
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub task_id: Uuid,
    pub role: String,
    pub content: String,
    pub execution_id: Option<Uuid>,
}
