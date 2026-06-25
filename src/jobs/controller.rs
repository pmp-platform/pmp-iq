//! The job controller: a leader-elected background loop that resumes paused
//! executions whose `resume_at` has elapsed. With multiple instances running,
//! only the leader (holder of the distributed lock) acts.

use super::clock::Clock;
use super::leader::LeaderLock;
use super::repository::JobExecutionRepository;
use super::runner::JobRunner;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const LOCK_NAME: &str = "job-controller";
const LEASE_SECONDS: i64 = 30;

/// Dependencies for the controller (bundled to bound parameter count).
#[derive(Clone)]
pub struct ControllerDeps {
    pub runner: Arc<JobRunner>,
    pub executions: Arc<dyn JobExecutionRepository>,
    pub lock: Arc<dyn LeaderLock>,
    pub clock: Arc<dyn Clock>,
}

/// Leader-elected controller that drives delayed resumes.
pub struct JobController {
    deps: ControllerDeps,
    holder: String,
    tick: Duration,
}

impl JobController {
    pub fn new(deps: ControllerDeps, tick: Duration) -> Self {
        Self {
            deps,
            holder: Uuid::new_v4().to_string(),
            tick,
        }
    }

    /// Try to become (or stay) the leader by acquiring/renewing the lock.
    pub async fn is_leader(&self) -> bool {
        let now = self.deps.clock.now();
        let expires = now + chrono::Duration::seconds(LEASE_SECONDS);
        self.deps
            .lock
            .try_acquire(LOCK_NAME, &self.holder, expires, now)
            .await
            .unwrap_or(false)
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

    /// Run the controller loop forever (leader-gated).
    pub async fn run(self: Arc<Self>) {
        loop {
            if self.is_leader().await {
                let resumed = self.resume_due().await;
                if resumed > 0 {
                    tracing::info!(resumed, "resumed paused executions");
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
    use crate::jobs::leader::MockLeaderLock;
    use crate::jobs::repository::MockJobExecutionRepository;
    use crate::jobs::{JobRunner, JobTypeRegistry, RunnerDeps};
    use crate::jobs::log_sink::MockLogSink;
    use crate::jobs::repository::MockJobRepository;
    use chrono::TimeZone;

    fn runner() -> Arc<JobRunner> {
        Arc::new(JobRunner::new(RunnerDeps {
            jobs: Arc::new(MockJobRepository::new()),
            executions: Arc::new(MockJobExecutionRepository::new()),
            registry: Arc::new(JobTypeRegistry::new()),
            clock: Arc::new(MockClock::new()),
            log_sink: Arc::new(MockLogSink::new()),
        }))
    }

    fn deps_with(lock: MockLeaderLock) -> ControllerDeps {
        let mut clock = MockClock::new();
        clock.expect_now().returning(|| chrono::Utc.timestamp_opt(100, 0).unwrap());
        let mut exec = MockJobExecutionRepository::new();
        exec.expect_list_due_resumes().returning(|_| Ok(vec![]));
        ControllerDeps {
            runner: runner(),
            executions: Arc::new(exec),
            lock: Arc::new(lock),
            clock: Arc::new(clock),
        }
    }

    #[tokio::test]
    async fn is_leader_reflects_lock_acquisition() {
        let mut lock = MockLeaderLock::new();
        lock.expect_try_acquire().returning(|_, _, _, _| Ok(true));
        let controller = JobController::new(deps_with(lock), Duration::from_secs(1));
        assert!(controller.is_leader().await);

        let mut lock2 = MockLeaderLock::new();
        lock2.expect_try_acquire().returning(|_, _, _, _| Ok(false));
        let controller2 = JobController::new(deps_with(lock2), Duration::from_secs(1));
        assert!(!controller2.is_leader().await);
    }

    #[tokio::test]
    async fn resume_due_handles_empty() {
        let mut lock = MockLeaderLock::new();
        lock.expect_try_acquire().returning(|_, _, _, _| Ok(true));
        let controller = JobController::new(deps_with(lock), Duration::from_secs(1));
        assert_eq!(controller.resume_due().await, 0);
    }
}
