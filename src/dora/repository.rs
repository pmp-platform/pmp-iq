//! Dual-engine persistence for DORA deployment + incident events (M47).

use super::model::{Deployment, Incident, NewDeployment};
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, SqlitePool};
use uuid::Uuid;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait DoraRepository: Send + Sync {
    async fn record_deployment(&self, input: NewDeployment) -> RepoResult<Deployment>;
    /// Open an incident, optionally attributing it to the deployment that caused it.
    async fn open_incident(&self, application_id: Uuid, caused_by: Option<Uuid>) -> RepoResult<Incident>;
    /// Mark an open incident resolved (sets `resolved_at`); idempotent.
    async fn resolve_incident(&self, id: Uuid) -> RepoResult<()>;
    async fn deployments_for(&self, application_id: Uuid, since: DateTime<Utc>) -> RepoResult<Vec<Deployment>>;
    async fn incidents_for(&self, application_id: Uuid, since: DateTime<Utc>) -> RepoResult<Vec<Incident>>;
    async fn all_deployments(&self, since: DateTime<Utc>) -> RepoResult<Vec<Deployment>>;
    async fn all_incidents(&self, since: DateTime<Utc>) -> RepoResult<Vec<Incident>>;
}

const DEPLOY_COLS: &str = "id, application_id, environment, sha, succeeded, deployed_at, first_commit_at";
const INCIDENT_COLS: &str = "id, application_id, caused_by, opened_at, resolved_at";

macro_rules! dora_impl {
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
        impl DoraRepository for $name {
            async fn record_deployment(&self, input: NewDeployment) -> RepoResult<Deployment> {
                let id = Uuid::new_v4();
                let deployed_at = Utc::now();
                sqlx::query(&$xform(
                    "INSERT INTO deployments (id, application_id, environment, sha, succeeded, deployed_at, first_commit_at) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7)",
                ))
                .bind(id)
                .bind(input.application_id)
                .bind(&input.environment)
                .bind(&input.sha)
                .bind(input.succeeded)
                .bind(deployed_at)
                .bind(input.first_commit_at)
                .execute(&self.pool)
                .await?;
                Ok(Deployment {
                    id,
                    application_id: Some(input.application_id),
                    environment: input.environment,
                    sha: input.sha,
                    succeeded: input.succeeded,
                    deployed_at,
                    first_commit_at: input.first_commit_at,
                })
            }

            async fn open_incident(&self, application_id: Uuid, caused_by: Option<Uuid>) -> RepoResult<Incident> {
                let id = Uuid::new_v4();
                let opened_at = Utc::now();
                sqlx::query(&$xform(
                    "INSERT INTO incidents (id, application_id, caused_by, opened_at) VALUES ($1,$2,$3,$4)",
                ))
                .bind(id)
                .bind(application_id)
                .bind(caused_by)
                .bind(opened_at)
                .execute(&self.pool)
                .await?;
                Ok(Incident { id, application_id: Some(application_id), caused_by, opened_at, resolved_at: None })
            }

            async fn resolve_incident(&self, id: Uuid) -> RepoResult<()> {
                sqlx::query(&$xform("UPDATE incidents SET resolved_at=$2 WHERE id=$1 AND resolved_at IS NULL"))
                    .bind(id)
                    .bind(Utc::now())
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }

            async fn deployments_for(&self, application_id: Uuid, since: DateTime<Utc>) -> RepoResult<Vec<Deployment>> {
                Ok(sqlx::query_as(&$xform(&format!(
                    "SELECT {DEPLOY_COLS} FROM deployments WHERE application_id=$1 AND deployed_at>=$2 \
                     ORDER BY deployed_at DESC"
                )))
                .bind(application_id)
                .bind(since)
                .fetch_all(&self.pool)
                .await?)
            }

            async fn incidents_for(&self, application_id: Uuid, since: DateTime<Utc>) -> RepoResult<Vec<Incident>> {
                Ok(sqlx::query_as(&$xform(&format!(
                    "SELECT {INCIDENT_COLS} FROM incidents WHERE application_id=$1 AND opened_at>=$2 \
                     ORDER BY opened_at DESC"
                )))
                .bind(application_id)
                .bind(since)
                .fetch_all(&self.pool)
                .await?)
            }

            async fn all_deployments(&self, since: DateTime<Utc>) -> RepoResult<Vec<Deployment>> {
                Ok(sqlx::query_as(&$xform(&format!(
                    "SELECT {DEPLOY_COLS} FROM deployments WHERE deployed_at>=$1 ORDER BY deployed_at DESC"
                )))
                .bind(since)
                .fetch_all(&self.pool)
                .await?)
            }

            async fn all_incidents(&self, since: DateTime<Utc>) -> RepoResult<Vec<Incident>> {
                Ok(sqlx::query_as(&$xform(&format!(
                    "SELECT {INCIDENT_COLS} FROM incidents WHERE opened_at>=$1 ORDER BY opened_at DESC"
                )))
                .bind(since)
                .fetch_all(&self.pool)
                .await?)
            }
        }
    };
}

dora_impl!(PgDoraRepository, PgPool, identity);
dora_impl!(SqliteDoraRepository, SqlitePool, to_sqlite);
