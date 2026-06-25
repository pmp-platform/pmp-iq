//! Integration tests for pausable/resumable jobs, the leader-elected resume
//! controller, and the distributed lock — exercised on SQLite (no container).

mod common;

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use common::SqliteDb;
use platform_inspector::db::Database;
use platform_inspector::jobs::clock::SystemClock;
use platform_inspector::jobs::controller::{ControllerDeps, JobController};
use platform_inspector::jobs::job_type::{JobContext, JobType, JobTypeRegistry};
use platform_inspector::jobs::model::{ExecStatus, JobInput, JobOutcome, TriggerType};
use platform_inspector::jobs::runner::{JobRunner, RunnerDeps};
use platform_inspector::jobs::JobError;
use platform_inspector::store;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// A job that pauses on its first run (with a resume time in the past) and
/// completes once resumed.
struct OncePauseJob;

#[async_trait]
impl JobType for OncePauseJob {
    fn id(&self) -> &str {
        "once-pause"
    }
    async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError> {
        if ctx.state.get("done").is_some() {
            ctx.log("resumed to completion").await;
            Ok(JobOutcome::completed(json!({ "resumed": true })))
        } else {
            ctx.log("self-pausing").await;
            Ok(JobOutcome::Paused {
                state: json!({ "done": true }),
                resume_at: Some(Utc::now() - ChronoDuration::seconds(1)),
            })
        }
    }
}

async fn poll_status(db: &Database, id: Uuid, want: ExecStatus) -> bool {
    let repo = store::job_executions(db);
    for _ in 0..50 {
        if let Ok(exec) = repo.get(id).await {
            if exec.status == want {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

fn runner(db: &Database, registry: JobTypeRegistry) -> Arc<JobRunner> {
    Arc::new(JobRunner::new(RunnerDeps {
        jobs: store::jobs(db),
        executions: store::job_executions(db),
        registry: Arc::new(registry),
        clock: Arc::new(SystemClock),
        log_sink: store::log_sink(db),
    }))
}

#[tokio::test]
async fn job_self_pauses_and_controller_resumes() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();

    let mut registry = JobTypeRegistry::new();
    registry.register(Arc::new(OncePauseJob));
    let runner = runner(&db, registry);

    // Configure and start the job.
    let job = store::jobs(&db)
        .create(JobInput {
            job_type: "once-pause".into(),
            name: "p".into(),
            trigger_type: TriggerType::Manual,
            cron_expr: None,
            config: json!({}),
            enabled: true,
        })
        .await
        .unwrap();
    let execution_id = runner.start(job.id, "manual").await.unwrap();

    // It self-pauses, persisting a checkpoint and a resume time.
    assert!(poll_status(&db, execution_id, ExecStatus::Paused).await, "should pause");
    let exec = store::job_executions(&db).get(execution_id).await.unwrap();
    assert_eq!(exec.state, Some(json!({ "done": true })));
    assert!(exec.resume_at.is_some());

    // The leader-elected controller resumes due executions.
    let controller = JobController::new(
        ControllerDeps {
            runner: runner.clone(),
            executions: store::job_executions(&db),
            lock: store::leader_lock(&db),
            clock: Arc::new(SystemClock),
        },
        Duration::from_millis(50),
    );
    assert!(controller.is_leader().await, "first instance is leader");
    let resumed = controller.resume_due().await;
    assert_eq!(resumed, 1);

    assert!(poll_status(&db, execution_id, ExecStatus::Succeeded).await, "should complete");
}

#[tokio::test]
async fn leader_lock_is_mutually_exclusive_until_expiry() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let lock = store::leader_lock(&db);

    let now = Utc::now();
    let lease = now + ChronoDuration::seconds(30);

    // First holder acquires.
    assert!(lock.try_acquire("ctrl", "A", lease, now).await.unwrap());
    // A renews (still holds).
    assert!(lock.try_acquire("ctrl", "A", lease, now).await.unwrap());
    // B cannot take it while A's lease is valid.
    assert!(!lock.try_acquire("ctrl", "B", lease, now).await.unwrap());
    // After A's lease expires, B can take over.
    let later = now + ChronoDuration::seconds(40);
    assert!(lock.try_acquire("ctrl", "B", later + ChronoDuration::seconds(30), later).await.unwrap());
    // Now A cannot reacquire while B holds.
    assert!(!lock.try_acquire("ctrl", "A", later + ChronoDuration::seconds(30), later).await.unwrap());
}
