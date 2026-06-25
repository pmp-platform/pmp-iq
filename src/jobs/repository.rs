//! Data access for jobs and executions.

use super::model::{
    ExecStatus, ExecutionUpdate, Job, JobExecution, JobInput, TriggerType,
};
use crate::db::{RepoError, RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{FromRow, PgPool, SqlitePool};
use uuid::Uuid;

/// CRUD access to configured jobs.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn create(&self, input: JobInput) -> RepoResult<Job>;
    async fn update(&self, id: Uuid, input: JobInput) -> RepoResult<Job>;
    async fn delete(&self, id: Uuid) -> RepoResult<()>;
    async fn get(&self, id: Uuid) -> RepoResult<Job>;
    async fn list(&self) -> RepoResult<Vec<Job>>;
    async fn list_enabled_cron(&self) -> RepoResult<Vec<Job>>;
}

/// Access to job executions.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait JobExecutionRepository: Send + Sync {
    async fn create(&self, job_id: Uuid, trigger: &str) -> RepoResult<JobExecution>;
    async fn update(&self, id: Uuid, update: ExecutionUpdate) -> RepoResult<()>;
    async fn get(&self, id: Uuid) -> RepoResult<JobExecution>;
    async fn list(&self, limit: i64) -> RepoResult<Vec<JobExecution>>;
    async fn count_running(&self, job_id: Uuid) -> RepoResult<i64>;
    /// Persist a job's resume checkpoint without changing status.
    async fn save_state(&self, id: Uuid, state: &Value) -> RepoResult<()>;
    /// Signal a running execution to pause cooperatively.
    async fn request_pause(&self, id: Uuid) -> RepoResult<()>;
    /// Move an execution to `paused`, recording its checkpoint and resume time.
    async fn mark_paused(
        &self,
        id: Uuid,
        state: Option<Value>,
        resume_at: Option<DateTime<Utc>>,
    ) -> RepoResult<()>;
    /// Paused executions whose `resume_at` has elapsed.
    async fn list_due_resumes(&self, now: DateTime<Utc>) -> RepoResult<Vec<JobExecution>>;
}

#[derive(FromRow)]
struct JobRow {
    id: Uuid,
    job_type: String,
    name: String,
    trigger_type: String,
    cron_expr: Option<String>,
    config: Value,
    enabled: bool,
}

impl TryFrom<JobRow> for Job {
    type Error = RepoError;
    fn try_from(row: JobRow) -> Result<Self, Self::Error> {
        Ok(Job {
            id: row.id,
            job_type: row.job_type,
            name: row.name,
            trigger_type: TriggerType::parse(&row.trigger_type).map_err(RepoError::Mapping)?,
            cron_expr: row.cron_expr,
            config: row.config,
            enabled: row.enabled,
        })
    }
}

#[derive(FromRow)]
struct ExecRow {
    id: Uuid,
    job_id: Uuid,
    status: String,
    trigger: String,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    summary: Option<Value>,
    error: Option<String>,
    logs: String,
    state: Option<Value>,
    resume_at: Option<DateTime<Utc>>,
    pause_requested: bool,
}

impl TryFrom<ExecRow> for JobExecution {
    type Error = RepoError;
    fn try_from(row: ExecRow) -> Result<Self, Self::Error> {
        Ok(JobExecution {
            id: row.id,
            job_id: row.job_id,
            status: ExecStatus::parse(&row.status).map_err(RepoError::Mapping)?,
            trigger: row.trigger,
            started_at: row.started_at,
            finished_at: row.finished_at,
            summary: row.summary,
            error: row.error,
            logs: row.logs,
            state: row.state,
            resume_at: row.resume_at,
            pause_requested: row.pause_requested,
        })
    }
}

const JOB_COLS: &str = "id, job_type, name, trigger_type, cron_expr, config, enabled";
const EXEC_COLS: &str = "id, job_id, status, trigger, started_at, finished_at, summary, error, \
     logs, state, resume_at, pause_requested";

macro_rules! job_repo_impl {
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
        impl JobRepository for $name {
            async fn create(&self, input: JobInput) -> RepoResult<Job> {
                let id = Uuid::new_v4();
                let row: JobRow = sqlx::query_as(&$xform(
                    "INSERT INTO jobs (id, job_type, name, trigger_type, cron_expr, config, enabled) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7) \
                     RETURNING id, job_type, name, trigger_type, cron_expr, config, enabled",
                ))
                .bind(id)
                .bind(&input.job_type)
                .bind(&input.name)
                .bind(input.trigger_type.as_str())
                .bind(&input.cron_expr)
                .bind(&input.config)
                .bind(input.enabled)
                .fetch_one(&self.pool)
                .await?;
                row.try_into()
            }

            async fn update(&self, id: Uuid, input: JobInput) -> RepoResult<Job> {
                let row: JobRow = sqlx::query_as(&$xform(
                    "UPDATE jobs SET job_type=$2, name=$3, trigger_type=$4, cron_expr=$5, config=$6, \
                     enabled=$7, updated_at=CURRENT_TIMESTAMP WHERE id=$1 \
                     RETURNING id, job_type, name, trigger_type, cron_expr, config, enabled",
                ))
                .bind(id)
                .bind(&input.job_type)
                .bind(&input.name)
                .bind(input.trigger_type.as_str())
                .bind(&input.cron_expr)
                .bind(&input.config)
                .bind(input.enabled)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;
                row.try_into()
            }

            async fn delete(&self, id: Uuid) -> RepoResult<()> {
                let res = sqlx::query(&$xform("DELETE FROM jobs WHERE id=$1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                if res.rows_affected() == 0 {
                    return Err(RepoError::NotFound);
                }
                Ok(())
            }

            async fn get(&self, id: Uuid) -> RepoResult<Job> {
                let row: JobRow = sqlx::query_as(&$xform(&format!(
                    "SELECT {JOB_COLS} FROM jobs WHERE id=$1"
                )))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;
                row.try_into()
            }

            async fn list(&self) -> RepoResult<Vec<Job>> {
                let rows: Vec<JobRow> = sqlx::query_as(&$xform(&format!(
                    "SELECT {JOB_COLS} FROM jobs ORDER BY name"
                )))
                .fetch_all(&self.pool)
                .await?;
                rows.into_iter().map(Job::try_from).collect()
            }

            async fn list_enabled_cron(&self) -> RepoResult<Vec<Job>> {
                let rows: Vec<JobRow> = sqlx::query_as(&$xform(&format!(
                    "SELECT {JOB_COLS} FROM jobs WHERE enabled AND trigger_type='cron' \
                     AND cron_expr IS NOT NULL"
                )))
                .fetch_all(&self.pool)
                .await?;
                rows.into_iter().map(Job::try_from).collect()
            }
        }
    };
}

macro_rules! exec_repo_impl {
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
        impl JobExecutionRepository for $name {
            async fn create(&self, job_id: Uuid, trigger: &str) -> RepoResult<JobExecution> {
                let id = Uuid::new_v4();
                let row: ExecRow = sqlx::query_as(&$xform(
                    "INSERT INTO job_executions (id, job_id, status, trigger) \
                     VALUES ($1,$2,'queued',$3) \
                     RETURNING id, job_id, status, trigger, started_at, finished_at, summary, error, \
                               logs, state, resume_at, pause_requested",
                ))
                .bind(id)
                .bind(job_id)
                .bind(trigger)
                .fetch_one(&self.pool)
                .await?;
                row.try_into()
            }

            async fn save_state(&self, id: Uuid, state: &Value) -> RepoResult<()> {
                sqlx::query(&$xform("UPDATE job_executions SET state=$2 WHERE id=$1"))
                    .bind(id)
                    .bind(state)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }

            async fn request_pause(&self, id: Uuid) -> RepoResult<()> {
                sqlx::query(&$xform("UPDATE job_executions SET pause_requested=$2 WHERE id=$1"))
                    .bind(id)
                    .bind(true)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }

            async fn mark_paused(
                &self,
                id: Uuid,
                state: Option<Value>,
                resume_at: Option<DateTime<Utc>>,
            ) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "UPDATE job_executions SET status='paused', state=$2, resume_at=$3, \
                     pause_requested=$4 WHERE id=$1",
                ))
                .bind(id)
                .bind(state)
                .bind(resume_at)
                .bind(false)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn list_due_resumes(&self, now: DateTime<Utc>) -> RepoResult<Vec<JobExecution>> {
                let rows: Vec<ExecRow> = sqlx::query_as(&$xform(&format!(
                    "SELECT {EXEC_COLS} FROM job_executions \
                     WHERE status='paused' AND resume_at IS NOT NULL AND resume_at <= $1 \
                     ORDER BY resume_at"
                )))
                .bind(now)
                .fetch_all(&self.pool)
                .await?;
                rows.into_iter().map(JobExecution::try_from).collect()
            }

            async fn update(&self, id: Uuid, update: ExecutionUpdate) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "UPDATE job_executions SET status=$2, started_at=COALESCE($3, started_at), \
                     finished_at=COALESCE($4, finished_at), summary=COALESCE($5, summary), \
                     error=COALESCE($6, error) WHERE id=$1",
                ))
                .bind(id)
                .bind(update.status.as_str())
                .bind(update.started_at)
                .bind(update.finished_at)
                .bind(update.summary)
                .bind(update.error)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn get(&self, id: Uuid) -> RepoResult<JobExecution> {
                let row: ExecRow = sqlx::query_as(&$xform(&format!(
                    "SELECT {EXEC_COLS} FROM job_executions WHERE id=$1"
                )))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;
                row.try_into()
            }

            async fn list(&self, limit: i64) -> RepoResult<Vec<JobExecution>> {
                let rows: Vec<ExecRow> = sqlx::query_as(&$xform(&format!(
                    "SELECT {EXEC_COLS} FROM job_executions ORDER BY created_at DESC LIMIT $1"
                )))
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
                rows.into_iter().map(JobExecution::try_from).collect()
            }

            async fn count_running(&self, job_id: Uuid) -> RepoResult<i64> {
                let (count,): (i64,) = sqlx::query_as(&$xform(
                    "SELECT COUNT(*) FROM job_executions WHERE job_id=$1 \
                     AND status IN ('queued','running')",
                ))
                .bind(job_id)
                .fetch_one(&self.pool)
                .await?;
                Ok(count)
            }
        }
    };
}

job_repo_impl!(PgJobRepository, PgPool, identity);
job_repo_impl!(SqliteJobRepository, SqlitePool, to_sqlite);
exec_repo_impl!(PgJobExecutionRepository, PgPool, identity);
exec_repo_impl!(SqliteJobExecutionRepository, SqlitePool, to_sqlite);
