//! Integration test for the M27 concurrency/queue repository methods on SQLite:
//! counting active vs in-flight executions, listing/claiming queued ones.

mod common;

use common::SqliteDb;
use pmp_iq::jobs::model::{JobInput, TriggerType};
use pmp_iq::store;
use serde_json::{Value, json};

#[tokio::test]
async fn queue_count_and_claim_roundtrip() {
    let db = SqliteDb::start().await;
    let jobs = store::jobs(&db.database());
    let execs = store::job_executions(&db.database());

    let job = jobs
        .create(JobInput {
            job_type: "noop".into(),
            name: "n".into(),
            trigger_type: TriggerType::Manual,
            cron_expr: None,
            config: json!({ "max_concurrency": 2 }),
            enabled: true,
            next_run_at: None,
        })
        .await
        .unwrap();

    // Three queued executions.
    let e1 = execs.create(job.id, "manual", &Value::Null).await.unwrap();
    let e2 = execs.create(job.id, "manual", &Value::Null).await.unwrap();
    let _e3 = execs.create(job.id, "manual", &Value::Null).await.unwrap();

    assert_eq!(execs.count_running(job.id).await.unwrap(), 3, "in-flight = queued+running");
    assert_eq!(execs.count_active(job.id).await.unwrap(), 0, "none running yet");
    assert_eq!(execs.count_all_active().await.unwrap(), 0);
    assert_eq!(execs.list_queued(10).await.unwrap().len(), 3);
    // FIFO: the oldest queued is e1.
    assert_eq!(execs.next_queued(job.id).await.unwrap().unwrap().id, e1.id);

    // Claim e1 (queued → running); a second claim of the same row loses.
    let now = chrono::Utc::now();
    assert!(execs.claim_queued(e1.id, now).await.unwrap(), "first claim wins");
    assert!(!execs.claim_queued(e1.id, now).await.unwrap(), "already running — re-claim fails");

    assert_eq!(execs.count_active(job.id).await.unwrap(), 1);
    assert_eq!(execs.count_all_active().await.unwrap(), 1);
    let queued = execs.list_queued(10).await.unwrap();
    assert_eq!(queued.len(), 2);
    // The next queued is now e2.
    assert_eq!(execs.next_queued(job.id).await.unwrap().unwrap().id, e2.id);
}
