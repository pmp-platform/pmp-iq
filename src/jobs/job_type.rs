//! Pluggable job types and their registry.

use super::log_sink::LogSink;
use super::model::{JobError, JobOutcome};
use super::repository::JobExecutionRepository;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Per-execution context handed to a job's `run` method.
pub struct JobContext {
    pub execution_id: Uuid,
    pub config: Value,
    /// Checkpoint persisted by a previous (paused) run; `Null` on a fresh start.
    pub state: Value,
    pub log: Arc<dyn LogSink>,
    pub executions: Arc<dyn JobExecutionRepository>,
}

impl JobContext {
    /// Convenience: emit a progress line (ignoring sink errors).
    pub async fn log(&self, message: &str) {
        let _ = self.log.append(self.execution_id, message).await;
    }

    /// Persist a resume checkpoint without pausing.
    pub async fn save_state(&self, state: &Value) {
        let _ = self.executions.save_state(self.execution_id, state).await;
    }

    /// Whether a manual pause has been requested for this execution.
    pub async fn pause_requested(&self) -> bool {
        self.executions
            .get(self.execution_id)
            .await
            .map(|e| e.pause_requested)
            .unwrap_or(false)
    }
}

/// A pluggable kind of job. Concrete implementations hold their own
/// dependencies and are registered once at startup.
#[async_trait]
pub trait JobType: Send + Sync {
    /// Stable type key matching `jobs.job_type`.
    fn id(&self) -> &str;

    /// Human-readable description shown when configuring a job.
    fn description(&self) -> &str {
        ""
    }

    /// Execute the job.
    async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError>;
}

/// Summary of a registered job type, for listing in the UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct JobTypeInfo {
    pub id: String,
    pub description: String,
}

/// Maps `job_type` keys to implementations.
#[derive(Default, Clone)]
pub struct JobTypeRegistry {
    types: HashMap<String, Arc<dyn JobType>>,
}

impl JobTypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, job_type: Arc<dyn JobType>) {
        self.types.insert(job_type.id().to_string(), job_type);
    }

    pub fn get(&self, job_type: &str) -> Option<Arc<dyn JobType>> {
        self.types.get(job_type).cloned()
    }

    pub fn keys(&self) -> Vec<String> {
        self.types.keys().cloned().collect()
    }

    /// All registered types with descriptions, sorted by id for stable display.
    pub fn list(&self) -> Vec<JobTypeInfo> {
        let mut out: Vec<JobTypeInfo> = self
            .types
            .values()
            .map(|t| JobTypeInfo {
                id: t.id().to_string(),
                description: t.description().to_string(),
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct StubJob;

    #[async_trait]
    impl JobType for StubJob {
        fn id(&self) -> &str {
            "stub"
        }
        async fn run(&self, _ctx: JobContext) -> Result<JobOutcome, JobError> {
            Ok(JobOutcome::completed(json!({})))
        }
    }

    struct DescribedJob;

    #[async_trait]
    impl JobType for DescribedJob {
        fn id(&self) -> &str {
            "alpha"
        }
        fn description(&self) -> &str {
            "does alpha things"
        }
        async fn run(&self, _ctx: JobContext) -> Result<JobOutcome, JobError> {
            Ok(JobOutcome::completed(json!({})))
        }
    }

    #[test]
    fn registry_registers_and_looks_up() {
        let mut registry = JobTypeRegistry::new();
        registry.register(Arc::new(StubJob));
        assert!(registry.get("stub").is_some());
        assert!(registry.get("missing").is_none());
        assert_eq!(registry.keys(), vec!["stub".to_string()]);
    }

    #[test]
    fn list_is_sorted_with_descriptions() {
        let mut registry = JobTypeRegistry::new();
        registry.register(Arc::new(StubJob));
        registry.register(Arc::new(DescribedJob));
        let list = registry.list();
        assert_eq!(list.len(), 2);
        // Sorted by id: "alpha" before "stub".
        assert_eq!(list[0].id, "alpha");
        assert_eq!(list[0].description, "does alpha things");
        assert_eq!(list[1].id, "stub");
        assert_eq!(list[1].description, ""); // default empty description
    }
}
