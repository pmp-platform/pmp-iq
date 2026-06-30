//! Integration test for the agent-task repository on PostgreSQL (covers
//! `PgAgentTaskRepository`), mirroring the SQLite smoke test.

mod common;

use common::TestDb;
use pmp_iq::agent_tasks::model::{NewAgentTask, NewMessage};
use pmp_iq::store;
use uuid::Uuid;

/// Insert a parent application (the FK on `agent_tasks.application_id`).
async fn seed_application(db: &TestDb) -> Uuid {
    let app_id = Uuid::new_v4();
    sqlx::query("INSERT INTO applications (id, name) VALUES ($1, $2)")
        .bind(app_id)
        .bind("demo-app")
        .execute(&db.pool)
        .await
        .unwrap();
    app_id
}

#[tokio::test]
async fn pg_create_transcript_and_status() {
    let db = TestDb::start().await;
    let app_id = seed_application(&db).await;
    let repo = store::agent_tasks(&db.database());

    let task = repo
        .create(NewAgentTask {
            application_id: app_id,
            repository_id: Uuid::new_v4(),
            title: "Add /health".into(),
        })
        .await
        .unwrap();
    assert_eq!(task.status, "draft");
    assert_eq!(task.branch_name, format!("agent/{}", task.id));

    // Round-trip with timestamps (TIMESTAMPTZ → DateTime<Utc>).
    let fetched = repo.get(task.id).await.unwrap();
    assert_eq!(fetched.created_at, task.created_at);
    assert_eq!(repo.list_for_application(app_id).await.unwrap().len(), 1);

    repo.add_message(NewMessage {
        task_id: task.id,
        role: "user".into(),
        content: "do it".into(),
        execution_id: Some(Uuid::new_v4()),
    })
    .await
    .unwrap();
    repo.add_message(NewMessage {
        task_id: task.id,
        role: "agent".into(),
        content: "opened PR".into(),
        execution_id: None,
    })
    .await
    .unwrap();
    let messages = repo.messages(task.id).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");

    repo.update_status(task.id, "pr_open", Some("https://example/pr/9".into()))
        .await
        .unwrap();
    let updated = repo.get(task.id).await.unwrap();
    assert_eq!(updated.status, "pr_open");
    assert_eq!(updated.pr_url.as_deref(), Some("https://example/pr/9"));

    // A None pr_url preserves the existing value (COALESCE).
    repo.update_status(task.id, "failed", None).await.unwrap();
    let again = repo.get(task.id).await.unwrap();
    assert_eq!(again.status, "failed");
    assert_eq!(again.pr_url.as_deref(), Some("https://example/pr/9"));
}
