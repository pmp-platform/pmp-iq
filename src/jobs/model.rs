//! Domain model for jobs and their executions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// How a job is triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerType {
    Manual,
    Cron,
}

impl TriggerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerType::Manual => "manual",
            TriggerType::Cron => "cron",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "manual" => Ok(TriggerType::Manual),
            "cron" => Ok(TriggerType::Cron),
            other => Err(format!("unknown trigger '{other}'")),
        }
    }
}

/// Lifecycle state of an execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecStatus {
    #[default]
    Queued,
    Running,
    Paused,
    Succeeded,
    Failed,
    Cancelled,
}

impl ExecStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecStatus::Queued => "queued",
            ExecStatus::Running => "running",
            ExecStatus::Paused => "paused",
            ExecStatus::Succeeded => "succeeded",
            ExecStatus::Failed => "failed",
            ExecStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "queued" => Ok(ExecStatus::Queued),
            "running" => Ok(ExecStatus::Running),
            "paused" => Ok(ExecStatus::Paused),
            "succeeded" => Ok(ExecStatus::Succeeded),
            "failed" => Ok(ExecStatus::Failed),
            "cancelled" => Ok(ExecStatus::Cancelled),
            other => Err(format!("unknown status '{other}'")),
        }
    }
}

/// A configured job.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: Uuid,
    pub job_type: String,
    pub name: String,
    pub trigger_type: TriggerType,
    pub cron_expr: Option<String>,
    pub config: Value,
    pub enabled: bool,
}

/// Fields needed to create/update a job.
#[derive(Debug, Clone)]
pub struct JobInput {
    pub job_type: String,
    pub name: String,
    pub trigger_type: TriggerType,
    pub cron_expr: Option<String>,
    pub config: Value,
    pub enabled: bool,
}

/// A job execution record.
#[derive(Debug, Clone, Serialize)]
pub struct JobExecution {
    pub id: Uuid,
    pub job_id: Uuid,
    pub status: ExecStatus,
    pub trigger: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub summary: Option<Value>,
    pub error: Option<String>,
    pub logs: String,
    /// Job-defined checkpoint to resume from.
    pub state: Option<Value>,
    /// When a self-paused execution should be resumed (null = manual/indefinite).
    pub resume_at: Option<DateTime<Utc>>,
    /// Cooperative manual-pause signal polled by a running job.
    pub pause_requested: bool,
}

/// Mutable fields applied to an execution as it progresses.
#[derive(Debug, Clone, Default)]
pub struct ExecutionUpdate {
    pub status: ExecStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub summary: Option<Value>,
    pub error: Option<String>,
}

/// Outcome of a job run: either completed with a summary, or paused with a
/// checkpoint to resume from later (optionally at `resume_at`).
#[derive(Debug, Clone)]
pub enum JobOutcome {
    Completed { summary: Value },
    Paused {
        state: Value,
        resume_at: Option<DateTime<Utc>>,
    },
}

impl JobOutcome {
    /// Convenience constructor for a completed run.
    pub fn completed(summary: Value) -> Self {
        JobOutcome::Completed { summary }
    }
}

/// Errors raised while running a job.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("job failed: {0}")]
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips() {
        for s in ["queued", "running", "succeeded", "failed", "cancelled"] {
            assert_eq!(ExecStatus::parse(s).unwrap().as_str(), s);
        }
        assert!(ExecStatus::parse("bogus").is_err());
    }

    #[test]
    fn trigger_round_trips() {
        for t in ["manual", "cron"] {
            assert_eq!(TriggerType::parse(t).unwrap().as_str(), t);
        }
    }
}
