//! Executes jobs, tracking status, timing, and errors.

use super::clock::Clock;
use super::job_type::{HeartbeatGuard, JobContext, JobTypeRegistry};
use super::log_sink::LogSink;
use super::model::{ExecStatus, ExecutionUpdate, Job, JobError, JobOutcome};
use super::repository::{JobExecutionRepository, JobRepository};
use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// How long to defer a job that declined to run without naming a retry time.
const DEFAULT_RESCHEDULE_MINUTES: i64 = 5;
/// Interval at which the runner heartbeats a running execution in the background.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);
/// Process-wide ceiling on concurrently-running executions across all jobs, to
/// bound resource use regardless of per-job `max_concurrency`.
const GLOBAL_MAX_ACTIVE: i64 = 16;

/// One execution to run (bundles inputs to bound parameter count).
pub struct ExecutionRun {
    pub execution_id: Uuid,
    pub job: Job,
    pub state: Value,
    pub params: Value,
}

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
        self.start_with_params(job_id, trigger, Value::Null).await
    }

    /// Start a job with per-execution input. Creates a queued execution and
    /// starts it immediately when the job has a free concurrency slot; otherwise
    /// it stays queued and the controller's dispatcher starts it when one frees.
    /// Returns the execution id (the caller polls it regardless).
    pub async fn start_with_params(
        self: &Arc<Self>,
        job_id: Uuid,
        trigger: &str,
        params: Value,
    ) -> Result<Uuid, AppError> {
        // Validate the job exists before enqueuing.
        self.deps.jobs.get(job_id).await?;
        let execution = self.deps.executions.create(job_id, trigger, &params).await?;
        self.dispatch_one(job_id).await;
        Ok(execution.id)
    }

    /// A job's configured max concurrent (running) executions (`config
    /// .max_concurrency`, default 1). Per-repo locks still serialise same-repo
    /// work regardless of this.
    pub fn job_max_concurrency(job: &Job) -> i64 {
        job.config
            .get("max_concurrency")
            .and_then(|v| v.as_i64())
            .filter(|&n| n > 0)
            .unwrap_or(1)
    }

    /// Start the oldest queued execution of `job_id` if the job (and the global
    /// cap) has a free slot. Returns whether one was started; a no-op (false)
    /// when at capacity or nothing is queued.
    pub async fn dispatch_one(self: &Arc<Self>, job_id: Uuid) -> bool {
        let job = match self.deps.jobs.get(job_id).await {
            Ok(job) => job,
            Err(_) => return false,
        };
        if self.deps.executions.count_all_active().await.unwrap_or(0) >= GLOBAL_MAX_ACTIVE {
            return false;
        }
        if self.deps.executions.count_active(job_id).await.unwrap_or(0)
            >= Self::job_max_concurrency(&job)
        {
            return false;
        }
        match self.deps.executions.next_queued(job_id).await {
            Ok(Some(exec)) => self.dispatch(exec.id).await,
            _ => false,
        }
    }

    /// Atomically claim a queued execution and run it in the background. Returns
    /// whether this caller won the claim (so two instances never double-start).
    pub async fn dispatch(self: &Arc<Self>, execution_id: Uuid) -> bool {
        let now = self.deps.clock.now();
        if !self
            .deps
            .executions
            .claim_queued(execution_id, now)
            .await
            .unwrap_or(false)
        {
            return false;
        }
        let exec = match self.deps.executions.get(execution_id).await {
            Ok(exec) => exec,
            Err(_) => return false,
        };
        let job = match self.deps.jobs.get(exec.job_id).await {
            Ok(job) => job,
            Err(_) => return false,
        };
        let state = exec.state.unwrap_or(Value::Null);
        let runner = Arc::clone(self);
        tokio::spawn(async move {
            runner
                .run_execution(ExecutionRun { execution_id, job, state, params: exec.params })
                .await;
        });
        true
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
            runner
                .run_execution(ExecutionRun { execution_id, job, state, params: exec.params })
                .await;
        });
        Ok(())
    }

    /// Execute a job inline, recording all status transitions (including pause
    /// and reschedule). Public so it can be unit-tested without spawning.
    pub async fn run_execution(&self, run: ExecutionRun) {
        let started = self.deps.clock.now();
        let _ = self
            .deps
            .executions
            .update(
                run.execution_id,
                ExecutionUpdate {
                    status: ExecStatus::Running,
                    started_at: Some(started),
                    heartbeat_at: Some(started),
                    ..Default::default()
                },
            )
            .await;

        // Heartbeat the execution in the background for its whole duration so a
        // long-but-healthy job is never cancelled mid-run (each job's heartbeat
        // is kept alive without the job having to remember). Stops when dropped.
        let heartbeat = HeartbeatGuard::spawn(
            self.deps.executions.clone(),
            self.deps.clock.clone(),
            run.execution_id,
            HEARTBEAT_INTERVAL,
        );
        let outcome = self.execute_job_type(&run).await;
        drop(heartbeat); // stop beating before the terminal transition

        // A stale-cancelled execution must not be overwritten with a terminal
        // status by the (now-stopping) job; only act while it is still running.
        if !self.still_running(run.execution_id).await {
            return;
        }
        match outcome {
            Ok(JobOutcome::Completed { summary }) => self.mark_done(&run, summary).await,
            Ok(JobOutcome::Paused { state, resume_at }) => {
                let _ = self
                    .deps
                    .executions
                    .mark_paused(run.execution_id, Some(state), resume_at)
                    .await;
            }
            Err(JobError::CannotRun { retry_at }) => self.reschedule(&run, retry_at).await,
            Err(JobError::Failed(message)) => self.mark_failed(&run, message).await,
        }
    }

    /// Whether the execution is still in the `running` state (i.e. not cancelled
    /// out from under the job by the stale-heartbeat sweep).
    async fn still_running(&self, execution_id: Uuid) -> bool {
        self.deps
            .executions
            .get(execution_id)
            .await
            .map(|e| e.status == ExecStatus::Running)
            .unwrap_or(false)
    }

    async fn mark_done(&self, run: &ExecutionRun, summary: Value) {
        let _ = self
            .deps
            .executions
            .update(
                run.execution_id,
                ExecutionUpdate {
                    status: ExecStatus::Succeeded,
                    finished_at: Some(self.deps.clock.now()),
                    summary: Some(summary),
                    ..Default::default()
                },
            )
            .await;
    }

    async fn mark_failed(&self, run: &ExecutionRun, message: String) {
        let _ = self
            .deps
            .executions
            .update(
                run.execution_id,
                ExecutionUpdate {
                    status: ExecStatus::Failed,
                    finished_at: Some(self.deps.clock.now()),
                    error: Some(message),
                    ..Default::default()
                },
            )
            .await;
    }

    /// The job declined to run: leave it pending (reschedule) without failing.
    async fn reschedule(&self, run: &ExecutionRun, retry_at: Option<DateTime<Utc>>) {
        let when = retry_at.unwrap_or_else(|| {
            self.deps.clock.now() + chrono::Duration::minutes(DEFAULT_RESCHEDULE_MINUTES)
        });
        let _ = self.deps.jobs.set_next_run_at(run.job.id, Some(when)).await;
        let _ = self
            .deps
            .executions
            .update(
                run.execution_id,
                ExecutionUpdate {
                    status: ExecStatus::Skipped,
                    finished_at: Some(self.deps.clock.now()),
                    ..Default::default()
                },
            )
            .await;
    }

    /// Resolve and run the job type, returning its outcome or a job error.
    async fn execute_job_type(&self, run: &ExecutionRun) -> Result<JobOutcome, JobError> {
        let job_type = self
            .deps
            .registry
            .get(&run.job.job_type)
            .ok_or_else(|| JobError::Failed(format!("unknown job type '{}'", run.job.job_type)))?;
        let ctx = JobContext {
            execution_id: run.execution_id,
            job_id: run.job.id,
            job_name: run.job.name.clone(),
            config: run.job.config.clone(),
            params: run.params.clone(),
            state: run.state.clone(),
            log: self.deps.log_sink.clone(),
            executions: self.deps.executions.clone(),
            clock: self.deps.clock.clone(),
        };
        job_type.run(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::clock::MockClock;
    use super::super::job_type::JobType;
    use super::super::log_sink::MockLogSink;
    use super::super::model::{JobExecution, JobOutcome, TriggerType};
    use super::super::repository::{MockJobExecutionRepository, MockJobRepository};
    use async_trait::async_trait;
    use chrono::TimeZone;
    use serde_json::json;

    fn exec_with(status: ExecStatus) -> JobExecution {
        JobExecution {
            id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            status,
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
            heartbeat_at: None,
        }
    }

    fn sample_job(job_type: &str) -> Job {
        Job {
            id: Uuid::new_v4(),
            job_type: job_type.into(),
            name: "t".into(),
            trigger_type: TriggerType::Manual,
            cron_expr: None,
            config: json!({}),
            enabled: true,
            next_run_at: None,
        }
    }

    fn run_of(job: Job) -> ExecutionRun {
        ExecutionRun { execution_id: Uuid::new_v4(), job, state: Value::Null, params: Value::Null }
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

    struct CannotRunJob;
    #[async_trait]
    impl JobType for CannotRunJob {
        fn id(&self) -> &str {
            "cannot"
        }
        async fn run(&self, _ctx: JobContext) -> Result<JobOutcome, JobError> {
            Err(JobError::CannotRun { retry_at: None })
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
        // The runner re-reads status before the terminal marking (cancel guard).
        executions.expect_get().returning(|_| Ok(exec_with(ExecStatus::Running)));
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
        runner.run_execution(run_of(sample_job("ok"))).await;
    }

    #[tokio::test]
    async fn failing_run_marks_failed() {
        let runner = JobRunner::new(deps_terminal(Arc::new(FailJob), ExecStatus::Failed));
        runner.run_execution(run_of(sample_job("fail"))).await;
    }

    #[tokio::test]
    async fn unknown_job_type_marks_failed() {
        let runner = JobRunner::new(deps_terminal(Arc::new(OkJob), ExecStatus::Failed));
        runner.run_execution(run_of(sample_job("missing"))).await;
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
        executions.expect_get().returning(|_| Ok(exec_with(ExecStatus::Running)));
        executions
            .expect_mark_paused()
            .withf(|_, state: &Option<Value>, resume_at: &Option<_>| {
                state.is_some() && resume_at.is_none()
            })
            .times(1)
            .returning(|_, _, _| Ok(()));
        let runner = JobRunner::new(deps(registry, clock, log, executions));
        runner.run_execution(run_of(sample_job("pause"))).await;
    }

    #[tokio::test]
    async fn cannot_run_reschedules_without_failing() {
        let (registry, clock, log) = base_mocks(Arc::new(CannotRunJob));
        let mut executions = MockJobExecutionRepository::new();
        executions
            .expect_update()
            .withf(|_, u: &ExecutionUpdate| u.status == ExecStatus::Running)
            .times(1)
            .returning(|_, _| Ok(()));
        executions.expect_get().returning(|_| Ok(exec_with(ExecStatus::Running)));
        // Rescheduled → execution is Skipped, not Failed.
        executions
            .expect_update()
            .withf(|_, u: &ExecutionUpdate| u.status == ExecStatus::Skipped && u.error.is_none())
            .times(1)
            .returning(|_, _| Ok(()));
        let mut jobs = MockJobRepository::new();
        jobs.expect_set_next_run_at()
            .withf(|_, when: &Option<_>| when.is_some())
            .times(1)
            .returning(|_, _| Ok(()));
        let runner = JobRunner::new(RunnerDeps {
            jobs: Arc::new(jobs),
            executions: Arc::new(executions),
            registry: Arc::new(registry),
            clock: Arc::new(clock),
            log_sink: Arc::new(log),
        });
        runner.run_execution(run_of(sample_job("cannot"))).await;
    }

    #[tokio::test]
    async fn cancelled_execution_is_not_overwritten() {
        let (registry, clock, log) = base_mocks(Arc::new(OkJob));
        let mut executions = MockJobExecutionRepository::new();
        // Only the initial Running transition; the job then completes but the
        // execution was already cancelled (stale), so no terminal update fires.
        executions
            .expect_update()
            .withf(|_, u: &ExecutionUpdate| u.status == ExecStatus::Running)
            .times(1)
            .returning(|_, _| Ok(()));
        executions.expect_get().returning(|_| Ok(exec_with(ExecStatus::Cancelled)));
        // No further `update` is expected (mockall fails if one occurs).
        let runner = JobRunner::new(deps(registry, clock, log, executions));
        runner.run_execution(run_of(sample_job("ok"))).await;
    }

    // --- M27: configurable concurrency / queue dispatch ----------------------

    fn job_with_concurrency(n: i64) -> Job {
        Job { config: json!({ "max_concurrency": n }), ..sample_job("agent") }
    }

    fn arc_runner(jobs: MockJobRepository, exec: MockJobExecutionRepository) -> Arc<JobRunner> {
        let mut clock = MockClock::new();
        clock.expect_now().returning(|| chrono::Utc.timestamp_opt(0, 0).unwrap());
        Arc::new(JobRunner::new(RunnerDeps {
            jobs: Arc::new(jobs),
            executions: Arc::new(exec),
            registry: Arc::new(JobTypeRegistry::new()),
            clock: Arc::new(clock),
            log_sink: Arc::new(MockLogSink::new()),
        }))
    }

    #[test]
    fn job_max_concurrency_reads_config_with_floor() {
        assert_eq!(JobRunner::job_max_concurrency(&job_with_concurrency(3)), 3);
        assert_eq!(JobRunner::job_max_concurrency(&sample_job("x")), 1); // default
        assert_eq!(JobRunner::job_max_concurrency(&job_with_concurrency(0)), 1); // floored to 1
    }

    #[tokio::test]
    async fn dispatch_one_skips_at_per_job_capacity() {
        let job = job_with_concurrency(2);
        let jid = job.id;
        let mut jobs = MockJobRepository::new();
        jobs.expect_get().returning(move |_| Ok(job.clone()));
        let mut exec = MockJobExecutionRepository::new();
        exec.expect_count_all_active().returning(|| Ok(0));
        exec.expect_count_active().returning(|_| Ok(2)); // at capacity
        exec.expect_next_queued().never(); // never reached
        let runner = arc_runner(jobs, exec);
        assert!(!runner.dispatch_one(jid).await);
    }

    #[tokio::test]
    async fn dispatch_one_skips_at_global_cap() {
        let job = job_with_concurrency(4);
        let jid = job.id;
        let mut jobs = MockJobRepository::new();
        jobs.expect_get().returning(move |_| Ok(job.clone()));
        let mut exec = MockJobExecutionRepository::new();
        exec.expect_count_all_active().returning(|| Ok(GLOBAL_MAX_ACTIVE));
        exec.expect_count_active().never(); // global cap short-circuits
        let runner = arc_runner(jobs, exec);
        assert!(!runner.dispatch_one(jid).await);
    }

    #[tokio::test]
    async fn dispatch_one_claims_queued_when_slot_free() {
        let job = job_with_concurrency(2);
        let jid = job.id;
        let mut jobs = MockJobRepository::new();
        jobs.expect_get().returning(move |_| Ok(job.clone()));
        let mut exec = MockJobExecutionRepository::new();
        exec.expect_count_all_active().returning(|| Ok(0));
        exec.expect_count_active().returning(|_| Ok(0)); // free slot
        exec.expect_next_queued().returning(|_| Ok(Some(exec_with(ExecStatus::Queued))));
        // Lost the atomic claim (another worker started it) → no spawn, returns false,
        // but the claim path was exercised.
        exec.expect_claim_queued().times(1).returning(|_, _| Ok(false));
        let runner = arc_runner(jobs, exec);
        assert!(!runner.dispatch_one(jid).await);
    }
}
