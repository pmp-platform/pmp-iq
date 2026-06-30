//! Integration test for the job heartbeat / staleness flow on SQLite: a running
//! execution heartbeats while alive, a stale heartbeat is found by `list_stale`,
//! `cancel` marks it cancelled, and a heartbeat after cancellation reports the
//! execution is no longer running (so the job stops itself).

mod common;

use chrono::{Duration, Utc};
use common::SqliteDb;
use pmp_iq::jobs::model::{ExecStatus, ExecutionUpdate, JobInput, TriggerType};
use pmp_iq::store;
use serde_json::{Value, json};

#[tokio::test]
async fn heartbeat_staleness_and_cancellation() {
    let sqlite = SqliteDb::start().await;
    let db = sqlite.database();
    let jobs = store::jobs(&db);
    let execs = store::job_executions(&db);

    let job = jobs
        .create(JobInput {
            job_type: "noop".into(),
            name: "hb".into(),
            trigger_type: TriggerType::Manual,
            cron_expr: None,
            config: json!({}),
            enabled: true,
            next_run_at: None,
        })
        .await
        .unwrap();
    let exec = execs.create(job.id, "manual", &Value::Null).await.unwrap();

    let now = Utc::now();
    execs
        .update(
            exec.id,
            ExecutionUpdate {
                status: ExecStatus::Running,
                started_at: Some(now),
                heartbeat_at: Some(now),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // A running execution heartbeats successfully; record an old heartbeat to
    // make it look stale.
    let stale_time = now - Duration::minutes(10);
    assert!(execs.heartbeat(exec.id, stale_time).await.unwrap(), "running → alive");

    // The stale sweep finds it (heartbeat older than the 5-minute cutoff).
    let cutoff = now - Duration::minutes(5);
    let stale = execs.list_stale(cutoff).await.unwrap();
    assert_eq!(stale.len(), 1, "stale execution is found");
    assert_eq!(stale[0].id, exec.id);

    // Cancelling it (as the controller would) flips it to cancelled...
    execs.cancel(exec.id, now, "stale: no heartbeat for 5 minutes").await.unwrap();
    let got = execs.get(exec.id).await.unwrap();
    assert_eq!(got.status, ExecStatus::Cancelled);
    assert_eq!(got.error.as_deref(), Some("stale: no heartbeat for 5 minutes"));

    // ...and a subsequent heartbeat reports it is no longer running, so the job
    // would notice and stop itself.
    assert!(!execs.heartbeat(exec.id, now).await.unwrap(), "cancelled → not alive");

    // A fresh heartbeat is no longer stale.
    assert!(execs.list_stale(cutoff).await.unwrap().is_empty(), "no longer running");
}
