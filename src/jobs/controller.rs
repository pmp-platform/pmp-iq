//! The job controller: a leader-elected background loop that resumes paused
//! executions whose `resume_at` has elapsed. With multiple instances running,
//! only the leader (holder of the distributed lock) acts.

use super::clock::Clock;
use super::repository::{JobExecutionRepository, JobRepository};
use super::runner::JobRunner;
use crate::locks::{DistributedLock, Lease, lock_keys};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const LEASE_SECONDS: u64 = 30;
/// A running execution that hasn't heartbeat within this window is stale.
const STALE_SECONDS: i64 = 300;

/// Dependencies for the controller (bundled to bound parameter count).
#[derive(Clone)]
pub struct ControllerDeps {
    pub runner: Arc<JobRunner>,
    pub jobs: Arc<dyn JobRepository>,
    pub executions: Arc<dyn JobExecutionRepository>,
    pub lock: Arc<dyn DistributedLock>,
    pub clock: Arc<dyn Clock>,
}

/// Leader-elected controller that drives delayed resumes.
pub struct JobController {
    deps: ControllerDeps,
    /// The held leader lease, if this instance is currently the leader.
    lease: Mutex<Option<Lease>>,
    tick: Duration,
}

impl JobController {
    pub fn new(deps: ControllerDeps, tick: Duration) -> Self {
        Self {
            deps,
            lease: Mutex::new(None),
            tick,
        }
    }

    /// Become (or stay) the leader by acquiring or renewing the controller lock.
    pub async fn is_leader(&self) -> bool {
        let ttl = Duration::from_secs(LEASE_SECONDS);
        let mut held = self.lease.lock().await;
        if let Some(current) = held.as_ref() {
            match self.deps.lock.refresh(current, ttl).await {
                Ok(renewed) => {
                    *held = Some(renewed);
                    return true;
                }
                Err(_) => *held = None,
            }
        }
        match self.deps.lock.acquire(&lock_keys::controller(), ttl).await {
            Ok(Some(lease)) => {
                *held = Some(lease);
                true
            }
            _ => false,
        }
    }

    /// Resume every paused execution whose `resume_at` has elapsed. Returns how
    /// many were resumed.
    pub async fn resume_due(&self) -> usize {
        let now = self.deps.clock.now();
        let due = self.deps.executions.list_due_resumes(now).await.unwrap_or_default();
        for exec in &due {
            if let Err(e) = self.deps.runner.resume(exec.id).await {
                tracing::warn!(execution = %exec.id, error = %e, "resume failed");
            }
        }
        due.len()
    }

    /// Start every enabled job whose `next_run_at` has elapsed. The job's
    /// `next_run_at` is cleared before starting (so it isn't re-picked); a job
    /// that declines to run re-sets it via the runner's reschedule path.
    pub async fn run_due_jobs(&self) -> usize {
        let now = self.deps.clock.now();
        let due = self.deps.jobs.list_due(now).await.unwrap_or_default();
        for job in &due {
            let _ = self.deps.jobs.set_next_run_at(job.id, None).await;
            if let Err(e) = self.deps.runner.start(job.id, "schedule").await {
                tracing::warn!(job = %job.id, error = %e, "scheduled start failed");
            }
        }
        due.len()
    }

    /// Cancel running executions whose heartbeat has gone stale (older than the
    /// stale window). Returns how many were cancelled. The running job notices
    /// at its next `ctx.heartbeat()` and stops itself.
    pub async fn cancel_stale(&self) -> usize {
        let now = self.deps.clock.now();
        let cutoff = now - chrono::Duration::seconds(STALE_SECONDS);
        let stale = self.deps.executions.list_stale(cutoff).await.unwrap_or_default();
        for exec in &stale {
            let reason = "stale: no heartbeat for 5 minutes";
            if let Err(e) = self.deps.executions.cancel(exec.id, now, reason).await {
                tracing::warn!(execution = %exec.id, error = %e, "cancel stale failed");
            }
        }
        stale.len()
    }

    /// Run the controller loop forever (leader-gated).
    pub async fn run(self: Arc<Self>) {
        loop {
            if self.is_leader().await {
                let resumed = self.resume_due().await;
                let started = self.run_due_jobs().await;
                let cancelled = self.cancel_stale().await;
                if resumed > 0 || started > 0 || cancelled > 0 {
                    tracing::info!(resumed, started, cancelled, "controller tick");
                }
            }
            tokio::time::sleep(self.tick).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::clock::MockClock;
    use crate::jobs::repository::MockJobExecutionRepository;
    use crate::jobs::{JobRunner, JobTypeRegistry, RunnerDeps};
    use crate::jobs::log_sink::MockLogSink;
    use crate::jobs::repository::MockJobRepository;
    use crate::locks::{Lease, MockDistributedLock};
    use chrono::{TimeZone, Utc};

    fn runner() -> Arc<JobRunner> {
        Arc::new(JobRunner::new(RunnerDeps {
            jobs: Arc::new(MockJobRepository::new()),
            executions: Arc::new(MockJobExecutionRepository::new()),
            registry: Arc::new(JobTypeRegistry::new()),
            clock: Arc::new(MockClock::new()),
            log_sink: Arc::new(MockLogSink::new()),
        }))
    }

    fn lease(key: &str) -> Lease {
        Lease { key: key.into(), token: "t".into(), expires_at: Utc.timestamp_opt(130, 0).unwrap() }
    }

    fn deps_with(lock: MockDistributedLock) -> ControllerDeps {
        let mut clock = MockClock::new();
        clock.expect_now().returning(|| chrono::Utc.timestamp_opt(100, 0).unwrap());
        let mut exec = MockJobExecutionRepository::new();
        exec.expect_list_due_resumes().returning(|_| Ok(vec![]));
        let mut jobs = MockJobRepository::new();
        jobs.expect_list_due().returning(|_| Ok(vec![]));
        ControllerDeps {
            runner: runner(),
            jobs: Arc::new(jobs),
            executions: Arc::new(exec),
            lock: Arc::new(lock),
            clock: Arc::new(clock),
        }
    }

    #[tokio::test]
    async fn is_leader_reflects_lock_acquisition() {
        let mut lock = MockDistributedLock::new();
        lock.expect_acquire().returning(|k, _| Ok(Some(lease(k))));
        let controller = JobController::new(deps_with(lock), Duration::from_secs(1));
        assert!(controller.is_leader().await);

        let mut lock2 = MockDistributedLock::new();
        lock2.expect_acquire().returning(|_, _| Ok(None));
        let controller2 = JobController::new(deps_with(lock2), Duration::from_secs(1));
        assert!(!controller2.is_leader().await);
    }

    #[tokio::test]
    async fn resume_due_handles_empty() {
        let mut lock = MockDistributedLock::new();
        lock.expect_acquire().returning(|k, _| Ok(Some(lease(k))));
        let controller = JobController::new(deps_with(lock), Duration::from_secs(1));
        assert_eq!(controller.resume_due().await, 0);
    }

    #[tokio::test]
    async fn cancel_stale_cancels_running_executions() {
        use crate::jobs::model::{ExecStatus, JobExecution};
        use serde_json::{Value, json};

        let stale = JobExecution {
            id: uuid::Uuid::new_v4(),
            job_id: uuid::Uuid::new_v4(),
            status: ExecStatus::Running,
            trigger: "manual".into(),
            started_at: None,
            finished_at: None,
            summary: None,
            error: None,
            logs: String::new(),
            state: None,
            resume_at: None,
            pause_requested: false,
            params: Value::Null,
            metadata: json!({}),
            heartbeat_at: Some(Utc.timestamp_opt(0, 0).unwrap()),
        };
        let mut clock = MockClock::new();
        clock.expect_now().returning(|| Utc.timestamp_opt(1000, 0).unwrap());
        let mut exec = MockJobExecutionRepository::new();
        exec.expect_list_stale().returning(move |_| Ok(vec![stale.clone()]));
        exec.expect_cancel().times(1).returning(|_, _, _| Ok(()));
        let deps = ControllerDeps {
            runner: runner(),
            jobs: Arc::new(MockJobRepository::new()),
            executions: Arc::new(exec),
            lock: Arc::new(MockDistributedLock::new()),
            clock: Arc::new(clock),
        };
        let controller = JobController::new(deps, Duration::from_secs(1));
        assert_eq!(controller.cancel_stale().await, 1);
    }
}
