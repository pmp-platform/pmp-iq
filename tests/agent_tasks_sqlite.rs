//! Smoke test for the agent-task repository against a real (in-memory) SQLite,
//! exercising the 019 migration plus UUID(BLOB)/timestamp(TEXT) decoding.

use platiq::agent_tasks::model::{NewAgentTask, NewAgentTaskTarget, NewMessage};
use platiq::agent_tasks::repository::{AgentTaskRepository, SqliteAgentTaskRepository};
use platiq::db::{Database, migrate};
use sqlx::sqlite::SqlitePoolOptions;
use uuid::Uuid;

/// Set up an in-memory SQLite, returning the repository and a parent
/// application id (the FK from `agent_tasks.application_id` is enforced).
async fn repo() -> (SqliteAgentTaskRepository, Uuid) {
    // max_connections(1) so every query shares the one in-memory database.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let db = Database::Sqlite(pool.clone());
    migrate::apply(&db, migrate::SQLITE_MIGRATIONS).await.unwrap();

    let app_id = Uuid::new_v4();
    sqlx::query("INSERT INTO applications (id, name) VALUES (?, ?)")
        .bind(app_id)
        .bind("demo-app")
        .execute(&pool)
        .await
        .unwrap();
    (SqliteAgentTaskRepository::new(pool), app_id)
}

#[tokio::test]
async fn create_get_list_and_transcript_roundtrip() {
    let (repo, app_id) = repo().await;

    let task = repo
        .create(NewAgentTask {
            application_id: app_id,
            repository_id: Uuid::new_v4(),
            title: "Add a /health endpoint".into(),
        })
        .await
        .unwrap();
    assert_eq!(task.title, "Add a /health endpoint");
    assert_eq!(task.status, "draft"); // DB default
    assert_eq!(task.branch_name, format!("agent/{}", task.id));
    assert!(task.pr_url.is_none());

    // Timestamps decode from SQLite TEXT into DateTime<Utc>.
    let fetched = repo.get(task.id).await.unwrap();
    assert_eq!(fetched.id, task.id);
    assert_eq!(fetched.created_at, task.created_at);

    let listed = repo.list_for_application(app_id).await.unwrap();
    assert_eq!(listed.len(), 1);

    repo.add_message(NewMessage {
        task_id: task.id,
        role: "user".into(),
        content: "please add it".into(),
        execution_id: Some(Uuid::new_v4()),
    })
    .await
    .unwrap();
    repo.add_message(NewMessage {
        task_id: task.id,
        role: "agent".into(),
        content: "done".into(),
        execution_id: None,
    })
    .await
    .unwrap();
    let messages = repo.messages(task.id).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");
    assert!(messages[0].execution_id.is_some());
    assert!(messages[1].execution_id.is_none());
}

#[tokio::test]
async fn update_status_sets_status_and_pr_url() {
    let (repo, app_id) = repo().await;
    let task = repo
        .create(NewAgentTask {
            application_id: app_id,
            repository_id: Uuid::new_v4(),
            title: "x".into(),
        })
        .await
        .unwrap();

    repo.update_status(task.id, "pr_open", Some("https://example/pr/1".into()))
        .await
        .unwrap();
    let updated = repo.get(task.id).await.unwrap();
    assert_eq!(updated.status, "pr_open");
    assert_eq!(updated.pr_url.as_deref(), Some("https://example/pr/1"));

    // A None pr_url keeps the existing value (COALESCE).
    repo.update_status(task.id, "failed", None).await.unwrap();
    let again = repo.get(task.id).await.unwrap();
    assert_eq!(again.status, "failed");
    assert_eq!(again.pr_url.as_deref(), Some("https://example/pr/1"));
}

#[tokio::test]
async fn target_crud_roundtrip() {
    let (repo, app_id) = repo().await;
    let task = repo
        .create(NewAgentTask {
            application_id: app_id,
            repository_id: Uuid::new_v4(),
            title: "multi".into(),
        })
        .await
        .unwrap();

    let t = repo
        .create_target(NewAgentTaskTarget {
            task_id: task.id,
            repository_id: Uuid::new_v4(),
            branch_name: task.branch_name.clone(),
        })
        .await
        .unwrap();
    assert_eq!(t.status, "pending");
    assert_eq!(repo.list_targets(task.id).await.unwrap().len(), 1);

    repo.update_target_status(t.id, "pr_open", Some("https://example/pr/2".into()))
        .await
        .unwrap();
    let got = repo.get_target(t.id).await.unwrap();
    assert_eq!(got.status, "pr_open");
    assert_eq!(got.pr_url.as_deref(), Some("https://example/pr/2"));
}
