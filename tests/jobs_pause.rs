//! Integration tests for pausable/resumable jobs, the leader-elected resume
//! controller, and the distributed lock — exercised on SQLite (no container).

mod common;

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, TimeZone, Utc};
use common::SqliteDb;
use pmp_iq::db::Database;
use pmp_iq::jobs::clock::{Clock, SystemClock};
use pmp_iq::jobs::controller::{ControllerDeps, JobController};
use pmp_iq::jobs::job_type::{JobContext, JobType, JobTypeRegistry};
use pmp_iq::jobs::model::{ExecStatus, JobInput, JobOutcome, TriggerType};
use pmp_iq::jobs::runner::{JobRunner, RunnerDeps};
use pmp_iq::jobs::JobError;
use pmp_iq::locks::{DistributedLock, SqliteSqlLock};
use pmp_iq::store;
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

/// A clock whose "now" can be advanced, for deterministic lease-expiry tests.
struct SettableClock(Mutex<DateTime<Utc>>);

impl SettableClock {
    fn new(secs: i64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(Utc.timestamp_opt(secs, 0).unwrap())))
    }
    fn advance(&self, secs: i64) {
        let mut g = self.0.lock().unwrap();
        *g += ChronoDuration::seconds(secs);
    }
}

impl Clock for SettableClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

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
            next_run_at: None,
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
            jobs: store::jobs(&db),
            executions: store::job_executions(&db),
            lock: store::distributed_lock(
                &db,
                &pmp_iq::config::RedisConfig {
                    enabled: false,
                    url: "redis://localhost:6379".into(),
                },
            )
            .unwrap(),
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
async fn distributed_lock_is_mutually_exclusive_until_expiry() {
    let sqlite = SqliteDb::start().await;
    let pool = match sqlite.database() {
        Database::Sqlite(pool) => pool,
        _ => unreachable!(),
    };
    let clock = SettableClock::new(100);
    let lock = SqliteSqlLock::new(pool, clock.clone());
    let ttl = Duration::from_secs(30);

    // First holder acquires.
    let a = lock.acquire("ctrl", ttl).await.unwrap().expect("A acquires");
    // A renews (still holds).
    assert!(lock.refresh(&a, ttl).await.is_ok());
    // A second, distinct caller cannot take it while A's lease is valid.
    assert!(lock.acquire("ctrl", ttl).await.unwrap().is_none());

    // After A's lease expires, another caller can take over.
    clock.advance(40);
    let b = lock.acquire("ctrl", ttl).await.unwrap().expect("B takes over");
    // A's stale lease can no longer be refreshed.
    assert!(lock.refresh(&a, ttl).await.is_err());

    // Releasing B frees the key immediately.
    lock.release(&b).await.unwrap();
    assert!(lock.acquire("ctrl", ttl).await.unwrap().is_some());
}
