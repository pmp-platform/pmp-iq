//! Pluggable job types and their registry.

use super::clock::Clock;
use super::log_sink::LogSink;
use super::model::{JobError, JobOutcome};
use super::recording::RecordingAiProvider;
use super::repository::JobExecutionRepository;
use crate::ai::AiProvider;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Per-execution context handed to a job's `run` method.
pub struct JobContext {
    pub execution_id: Uuid,
    /// The owning job's id and name (used for stable per-job workspace paths).
    pub job_id: Uuid,
    pub job_name: String,
    pub config: Value,
    /// Per-execution input (e.g. an ad-hoc LLM question); `Null` when unused.
    pub params: Value,
    /// Checkpoint persisted by a previous (paused) run; `Null` on a fresh start.
    pub state: Value,
    pub log: Arc<dyn LogSink>,
    pub executions: Arc<dyn JobExecutionRepository>,
    pub clock: Arc<dyn Clock>,
}

impl JobContext {
    /// Convenience: emit a progress line (ignoring sink errors).
    pub async fn log(&self, message: &str) {
        let _ = self.log.append(self.execution_id, message).await;
    }

    /// Append raw text to the execution's live output (the `logs` channel).
    pub async fn append_output(&self, text: &str) {
        let _ = self.log.append(self.execution_id, text).await;
    }

    /// Deep-merge a JSON patch into the execution's metadata.
    pub async fn merge_metadata(&self, patch: &Value) {
        let _ = self.executions.merge_metadata(self.execution_id, patch).await;
    }

    /// Wrap an AI provider so its full input/output is written to the execution
    /// output and token usage is accumulated into the execution metadata.
    pub fn recording_provider(&self, inner: Box<dyn AiProvider>) -> RecordingAiProvider {
        RecordingAiProvider::new(
            inner,
            self.execution_id,
            self.log.clone(),
            self.executions.clone(),
        )
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

    /// Record a liveness heartbeat. Returns `true` while the execution is still
    /// running, and `false` once it has been cancelled (so a long-running job
    /// that called this can notice it was cancelled and stop itself).
    pub async fn heartbeat(&self) -> bool {
        self.executions
            .heartbeat(self.execution_id, self.clock.now())
            .await
            .unwrap_or(false)
    }

    /// Spawn a background heartbeat that beats every `interval` until dropped
    /// (or the execution is cancelled). Use it around a single long operation
    /// (e.g. an LLM call) that has no natural progress points to heartbeat at.
    pub fn heartbeat_guard(&self, interval: Duration) -> HeartbeatGuard {
        HeartbeatGuard::spawn(self.executions.clone(), self.clock.clone(), self.execution_id, interval)
    }
}

/// A background task that heartbeats an execution at a fixed interval, stopping
/// when dropped (or once the execution is no longer running). The runner holds
/// one for every execution so a long-but-healthy job is never cancelled
/// mid-run; staleness then only catches executions whose worker has died.
pub struct HeartbeatGuard {
    handle: tokio::task::JoinHandle<()>,
}

impl HeartbeatGuard {
    pub fn spawn(
        executions: Arc<dyn JobExecutionRepository>,
        clock: Arc<dyn Clock>,
        execution_id: Uuid,
        interval: Duration,
    ) -> Self {
        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                // Stop once the execution is no longer running (cancelled/done).
                if !executions.heartbeat(execution_id, clock.now()).await.unwrap_or(false) {
                    break;
                }
            }
        });
        HeartbeatGuard { handle }
    }
}

impl Drop for HeartbeatGuard {
    fn drop(&mut self) {
        self.handle.abort();
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
    use crate::jobs::clock::MockClock;
    use crate::jobs::repository::MockJobExecutionRepository;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn heartbeat_guard_beats_until_dropped() {
        let beats = Arc::new(AtomicUsize::new(0));
        let counter = beats.clone();
        let mut executions = MockJobExecutionRepository::new();
        executions.expect_heartbeat().returning(move |_, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        });
        let mut clock = MockClock::new();
        clock.expect_now().returning(chrono::Utc::now);

        let guard = HeartbeatGuard::spawn(
            Arc::new(executions),
            Arc::new(clock),
            Uuid::new_v4(),
            Duration::from_millis(10),
        );
        tokio::time::sleep(Duration::from_millis(55)).await;
        let while_alive = beats.load(Ordering::SeqCst);
        assert!(while_alive >= 2, "guard beats while alive: {while_alive}");

        // Dropping the guard stops the background heartbeat.
        drop(guard);
        tokio::time::sleep(Duration::from_millis(40)).await;
        let after_drop = beats.load(Ordering::SeqCst);
        assert!(after_drop <= while_alive + 1, "stopped after drop: {while_alive} → {after_drop}");
    }

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
