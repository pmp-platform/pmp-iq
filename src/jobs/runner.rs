//! Executes jobs, tracking status, timing, and errors.

use super::clock::Clock;
use super::job_type::{JobContext, JobTypeRegistry};
use super::log_sink::LogSink;
use super::model::{ExecStatus, ExecutionUpdate, Job, JobError, JobOutcome};
use super::repository::{JobExecutionRepository, JobRepository};
use crate::error::AppError;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

/// Bundled dependencies for the runner (keeps construction within the parameter
/// limit).
#[derive(Clone)]
pub struct RunnerDeps {
    pub jobs: Arc<dyn JobRepository>,
    pub executions: Arc<dyn JobExecutionRepository>,
    pub registry: Arc<JobTypeRegistry>,
    pub clock: Arc<dyn Clock>,
    pub log_sink: Arc<dyn LogSink>,
}

/// Runs jobs and records their executions.
#[derive(Clone)]
pub struct JobRunner {
    deps: RunnerDeps,
}

impl JobRunner {
    pub fn new(deps: RunnerDeps) -> Self {
        Self { deps }
    }

    /// Start a job: create a queued execution and run it in the background.
    /// Returns the execution id. Fails if the job is already in flight.
    pub async fn start(self: &Arc<Self>, job_id: Uuid, trigger: &str) -> Result<Uuid, AppError> {
        let job = self.deps.jobs.get(job_id).await?;
        if self.deps.executions.count_running(job_id).await? > 0 {
            return Err(AppError::Conflict("job already running".into()));
        }
        let execution = self.deps.executions.create(job_id, trigger).await?;
        let execution_id = execution.id;

        let runner = Arc::clone(self);
        tokio::spawn(async move {
            runner.run_execution(execution_id, job, Value::Null).await;
        });
        Ok(execution_id)
    }

    /// Resume a paused execution from its persisted checkpoint, in the
    /// background.
    pub async fn resume(self: &Arc<Self>, execution_id: Uuid) -> Result<(), AppError> {
        let exec = self.deps.executions.get(execution_id).await?;
        if exec.status != ExecStatus::Paused {
            return Err(AppError::BadRequest("execution is not paused".into()));
        }
        let job = self.deps.jobs.get(exec.job_id).await?;
        let state = exec.state.unwrap_or(Value::Null);
        let runner = Arc::clone(self);
        tokio::spawn(async move {
            runner.run_execution(execution_id, job, state).await;
        });
        Ok(())
    }

    /// Execute a job inline, recording all status transitions (including pause).
    /// Public so it can be unit-tested without spawning.
    pub async fn run_execution(&self, execution_id: Uuid, job: Job, state: Value) {
        let started = self.deps.clock.now();
        let _ = self
            .deps
            .executions
            .update(
                execution_id,
                ExecutionUpdate {
                    status: ExecStatus::Running,
                    started_at: Some(started),
                    ..Default::default()
                },
            )
            .await;

        match self.execute_job_type(execution_id, &job, state).await {
            Ok(JobOutcome::Completed { summary }) => {
                let finished = self.deps.clock.now();
                let _ = self
                    .deps
                    .executions
                    .update(
                        execution_id,
                        ExecutionUpdate {
                            status: ExecStatus::Succeeded,
                            finished_at: Some(finished),
                            summary: Some(summary),
                            ..Default::default()
                        },
                    )
                    .await;
            }
            Ok(JobOutcome::Paused { state, resume_at }) => {
                let _ = self
                    .deps
                    .executions
                    .mark_paused(execution_id, Some(state), resume_at)
                    .await;
            }
            Err(message) => {
                let finished = self.deps.clock.now();
                let _ = self
                    .deps
                    .executions
                    .update(
                        execution_id,
                        ExecutionUpdate {
                            status: ExecStatus::Failed,
                            finished_at: Some(finished),
                            error: Some(message),
                            ..Default::default()
                        },
                    )
                    .await;
            }
        }
    }

    /// Resolve and run the job type, returning its outcome or an error message.
    async fn execute_job_type(
        &self,
        execution_id: Uuid,
        job: &Job,
        state: Value,
    ) -> Result<JobOutcome, String> {
        let job_type = self
            .deps
            .registry
            .get(&job.job_type)
            .ok_or_else(|| format!("unknown job type '{}'", job.job_type))?;
        let ctx = JobContext {
            execution_id,
            config: job.config.clone(),
            state,
            log: self.deps.log_sink.clone(),
            executions: self.deps.executions.clone(),
        };
        job_type
            .run(ctx)
            .await
            .map_err(|JobError::Failed(message)| message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::clock::MockClock;
    use super::super::job_type::JobType;
    use super::super::log_sink::MockLogSink;
    use super::super::model::{JobOutcome, TriggerType};
    use super::super::repository::{MockJobExecutionRepository, MockJobRepository};
    use async_trait::async_trait;
    use chrono::TimeZone;
    use serde_json::json;

    fn sample_job(job_type: &str) -> Job {
        Job {
            id: Uuid::new_v4(),
            job_type: job_type.into(),
            name: "t".into(),
            trigger_type: TriggerType::Manual,
            cron_expr: None,
            config: json!({}),
            enabled: true,
        }
    }

    struct OkJob;
    #[async_trait]
    impl JobType for OkJob {
        fn id(&self) -> &str {
            "ok"
        }
        async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError> {
            ctx.log("working").await;
            Ok(JobOutcome::completed(json!({ "done": true })))
        }
    }

    struct FailJob;
    #[async_trait]
    impl JobType for FailJob {
        fn id(&self) -> &str {
            "fail"
        }
        async fn run(&self, _ctx: JobContext) -> Result<JobOutcome, JobError> {
            Err(JobError::Failed("boom".into()))
        }
    }

    struct PauseJob;
    #[async_trait]
    impl JobType for PauseJob {
        fn id(&self) -> &str {
            "pause"
        }
        async fn run(&self, _ctx: JobContext) -> Result<JobOutcome, JobError> {
            Ok(JobOutcome::Paused {
                state: json!({ "step": 1 }),
                resume_at: None,
            })
        }
    }

    fn base_mocks(
        job_type: Arc<dyn JobType>,
    ) -> (JobTypeRegistry, MockClock, MockLogSink) {
        let mut registry = JobTypeRegistry::new();
        registry.register(job_type);
        let mut clock = MockClock::new();
        clock.expect_now().returning(|| chrono::Utc.timestamp_opt(0, 0).unwrap());
        let mut log = MockLogSink::new();
        log.expect_append().returning(|_, _| Ok(()));
        (registry, clock, log)
    }

    fn deps(registry: JobTypeRegistry, clock: MockClock, log: MockLogSink, exec: MockJobExecutionRepository) -> RunnerDeps {
        RunnerDeps {
            jobs: Arc::new(MockJobRepository::new()),
            executions: Arc::new(exec),
            registry: Arc::new(registry),
            clock: Arc::new(clock),
            log_sink: Arc::new(log),
        }
    }

    fn deps_terminal(job_type: Arc<dyn JobType>, expect_status: ExecStatus) -> RunnerDeps {
        let (registry, clock, log) = base_mocks(job_type);
        let mut executions = MockJobExecutionRepository::new();
        executions
            .expect_update()
            .withf(|_, u: &ExecutionUpdate| u.status == ExecStatus::Running)
            .times(1)
            .returning(|_, _| Ok(()));
        executions
            .expect_update()
            .withf(move |_, u: &ExecutionUpdate| u.status == expect_status)
            .times(1)
            .returning(|_, _| Ok(()));
        deps(registry, clock, log, executions)
    }

    #[tokio::test]
    async fn successful_run_marks_succeeded() {
        let runner = JobRunner::new(deps_terminal(Arc::new(OkJob), ExecStatus::Succeeded));
        runner.run_execution(Uuid::new_v4(), sample_job("ok"), Value::Null).await;
    }

    #[tokio::test]
    async fn failing_run_marks_failed() {
        let runner = JobRunner::new(deps_terminal(Arc::new(FailJob), ExecStatus::Failed));
        runner.run_execution(Uuid::new_v4(), sample_job("fail"), Value::Null).await;
    }

    #[tokio::test]
    async fn unknown_job_type_marks_failed() {
        let runner = JobRunner::new(deps_terminal(Arc::new(OkJob), ExecStatus::Failed));
        runner.run_execution(Uuid::new_v4(), sample_job("missing"), Value::Null).await;
    }

    #[tokio::test]
    async fn paused_run_records_checkpoint() {
        let (registry, clock, log) = base_mocks(Arc::new(PauseJob));
        let mut executions = MockJobExecutionRepository::new();
        executions
            .expect_update()
            .withf(|_, u: &ExecutionUpdate| u.status == ExecStatus::Running)
            .times(1)
            .returning(|_, _| Ok(()));
        executions
            .expect_mark_paused()
            .withf(|_, state: &Option<Value>, resume_at: &Option<_>| {
                state.is_some() && resume_at.is_none()
            })
            .times(1)
            .returning(|_, _, _| Ok(()));
        let runner = JobRunner::new(deps(registry, clock, log, executions));
        runner.run_execution(Uuid::new_v4(), sample_job("pause"), Value::Null).await;
    }
}
