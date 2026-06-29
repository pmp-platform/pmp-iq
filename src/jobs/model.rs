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
    /// The job declined to run now and was rescheduled (not an error).
    Skipped,
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
            ExecStatus::Skipped => "skipped",
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
            "skipped" => Ok(ExecStatus::Skipped),
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
    /// When the scheduler should next run this job (UTC); `None` = not scheduled.
    pub next_run_at: Option<DateTime<Utc>>,
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
    pub next_run_at: Option<DateTime<Utc>>,
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
    /// Per-execution input (e.g. an ad-hoc LLM question).
    pub params: Value,
    /// Structured metadata the job updates while running / returns when done.
    pub metadata: Value,
    /// Last liveness heartbeat from the running job (null until it starts).
    pub heartbeat_at: Option<DateTime<Utc>>,
}

/// Mutable fields applied to an execution as it progresses.
#[derive(Debug, Clone, Default)]
pub struct ExecutionUpdate {
    pub status: ExecStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub summary: Option<Value>,
    pub error: Option<String>,
    pub heartbeat_at: Option<DateTime<Utc>>,
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
    /// The job could not run now (e.g. it couldn't take its lock). The runner
    /// reschedules it (at `retry_at`, or `now + 5m`) rather than marking it
    /// failed.
    #[error("job cannot run now")]
    CannotRun { retry_at: Option<DateTime<Utc>> },
}

/// Deep-merge `patch` into `base` (object keys merged recursively; non-object
/// values replaced). Used for incremental execution metadata updates.
pub fn merge_object(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (key, value) in p {
                merge_object(b.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (base, patch) => *base = patch.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips() {
        for s in ["queued", "running", "paused", "succeeded", "failed", "cancelled", "skipped"] {
            assert_eq!(ExecStatus::parse(s).unwrap().as_str(), s);
        }
        assert!(ExecStatus::parse("bogus").is_err());
    }

    #[test]
    fn merge_object_deep_merges_and_replaces_leaves() {
        use serde_json::json;
        let mut base = json!({ "llm": { "calls": 1 }, "keep": true });
        merge_object(&mut base, &json!({ "llm": { "calls": 2, "tokens": 10 } }));
        assert_eq!(base, json!({ "llm": { "calls": 2, "tokens": 10 }, "keep": true }));
    }

    #[test]
    fn trigger_round_trips() {
        for t in ["manual", "cron"] {
            assert_eq!(TriggerType::parse(t).unwrap().as_str(), t);
        }
    }
}
