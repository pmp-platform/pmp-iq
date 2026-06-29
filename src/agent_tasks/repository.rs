//! Dual-engine persistence for AI Agent tasks and their messages.

use super::model::{AgentTask, AgentTaskMessage, NewAgentTask, NewMessage};
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, SqlitePool};
use uuid::Uuid;

/// CRUD access to agent tasks + their transcripts.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait AgentTaskRepository: Send + Sync {
    async fn create(&self, input: NewAgentTask) -> RepoResult<AgentTask>;
    async fn get(&self, id: Uuid) -> RepoResult<AgentTask>;
    async fn list_for_application(&self, application_id: Uuid) -> RepoResult<Vec<AgentTask>>;
    async fn add_message(&self, input: NewMessage) -> RepoResult<AgentTaskMessage>;
    async fn messages(&self, task_id: Uuid) -> RepoResult<Vec<AgentTaskMessage>>;
    async fn update_status(&self, id: Uuid, status: &str, pr_url: Option<String>)
    -> RepoResult<()>;
}

#[derive(FromRow)]
struct TaskRow {
    id: Uuid,
    application_id: Uuid,
    repository_id: Uuid,
    title: String,
    status: String,
    branch_name: String,
    pr_url: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<TaskRow> for AgentTask {
    fn from(r: TaskRow) -> Self {
        AgentTask {
            id: r.id,
            application_id: r.application_id,
            repository_id: r.repository_id,
            title: r.title,
            status: r.status,
            branch_name: r.branch_name,
            pr_url: r.pr_url,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(FromRow)]
struct MessageRow {
    id: Uuid,
    task_id: Uuid,
    role: String,
    content: String,
    execution_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

impl From<MessageRow> for AgentTaskMessage {
    fn from(r: MessageRow) -> Self {
        AgentTaskMessage {
            id: r.id,
            task_id: r.task_id,
            role: r.role,
            content: r.content,
            execution_id: r.execution_id,
            created_at: r.created_at,
        }
    }
}

const TASK_COLS: &str =
    "id, application_id, repository_id, title, status, branch_name, pr_url, created_at, updated_at";
const MSG_COLS: &str = "id, task_id, role, content, execution_id, created_at";

macro_rules! agent_task_impl {
    ($name:ident, $pool:ty, $xform:path) => {
        pub struct $name {
            pool: $pool,
        }
        impl $name {
            pub fn new(pool: $pool) -> Self {
                Self { pool }
            }
        }
        #[async_trait]
        impl AgentTaskRepository for $name {
            async fn create(&self, input: NewAgentTask) -> RepoResult<AgentTask> {
                let id = Uuid::new_v4();
                let branch = format!("agent/{id}");
                let row: TaskRow = sqlx::query_as(&$xform(&format!(
                    "INSERT INTO agent_tasks \
                       (id, application_id, repository_id, title, branch_name) \
                     VALUES ($1,$2,$3,$4,$5) RETURNING {TASK_COLS}"
                )))
                .bind(id)
                .bind(input.application_id)
                .bind(input.repository_id)
                .bind(&input.title)
                .bind(&branch)
                .fetch_one(&self.pool)
                .await?;
                Ok(row.into())
            }

            async fn get(&self, id: Uuid) -> RepoResult<AgentTask> {
                let row: TaskRow = sqlx::query_as(&$xform(&format!(
                    "SELECT {TASK_COLS} FROM agent_tasks WHERE id=$1"
                )))
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
                Ok(row.into())
            }

            async fn list_for_application(
                &self,
                application_id: Uuid,
            ) -> RepoResult<Vec<AgentTask>> {
                let rows: Vec<TaskRow> = sqlx::query_as(&$xform(&format!(
                    "SELECT {TASK_COLS} FROM agent_tasks WHERE application_id=$1 \
                     ORDER BY created_at DESC"
                )))
                .bind(application_id)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(AgentTask::from).collect())
            }

            async fn add_message(&self, input: NewMessage) -> RepoResult<AgentTaskMessage> {
                let id = Uuid::new_v4();
                let row: MessageRow = sqlx::query_as(&$xform(&format!(
                    "INSERT INTO agent_task_messages (id, task_id, role, content, execution_id) \
                     VALUES ($1,$2,$3,$4,$5) RETURNING {MSG_COLS}"
                )))
                .bind(id)
                .bind(input.task_id)
                .bind(&input.role)
                .bind(&input.content)
                .bind(input.execution_id)
                .fetch_one(&self.pool)
                .await?;
                Ok(row.into())
            }

            async fn messages(&self, task_id: Uuid) -> RepoResult<Vec<AgentTaskMessage>> {
                let rows: Vec<MessageRow> = sqlx::query_as(&$xform(&format!(
                    "SELECT {MSG_COLS} FROM agent_task_messages WHERE task_id=$1 \
                     ORDER BY created_at"
                )))
                .bind(task_id)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(AgentTaskMessage::from).collect())
            }

            async fn update_status(
                &self,
                id: Uuid,
                status: &str,
                pr_url: Option<String>,
            ) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "UPDATE agent_tasks SET status=$2, pr_url=COALESCE($3, pr_url), \
                     updated_at=CURRENT_TIMESTAMP WHERE id=$1",
                ))
                .bind(id)
                .bind(status)
                .bind(pr_url)
                .execute(&self.pool)
                .await?;
                Ok(())
            }
        }
    };
}

agent_task_impl!(PgAgentTaskRepository, PgPool, identity);
agent_task_impl!(SqliteAgentTaskRepository, SqlitePool, to_sqlite);
