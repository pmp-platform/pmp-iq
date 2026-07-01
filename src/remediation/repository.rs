//! Dual-engine persistence for remediation rules + emitted remediations (M46).

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgPool, SqlitePool};
use uuid::Uuid;

/// A rule mapping a finding trigger to a remediation action.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RemediationRule {
    pub id: Uuid,
    pub name: String,
    pub trigger_kind: String,
    pub params: Value,
    pub action: String,
    pub prompt: String,
    pub scope: Value,
    pub auto_approve: bool,
    pub enabled: bool,
}

/// Fields to create a rule.
#[derive(Debug, Clone)]
pub struct RuleInput {
    pub name: String,
    pub trigger_kind: String,
    pub params: Value,
    pub action: String,
    pub prompt: String,
    pub scope: Value,
    pub auto_approve: bool,
}

/// One emitted remediation.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Remediation {
    pub id: Uuid,
    pub rule_id: Option<Uuid>,
    pub application_id: Option<Uuid>,
    pub finding_key: String,
    pub status: String,
    pub agent_task_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RemediationRepository: Send + Sync {
    async fn list_rules(&self) -> RepoResult<Vec<RemediationRule>>;
    async fn create_rule(&self, input: RuleInput) -> RepoResult<RemediationRule>;
    async fn delete_rule(&self, id: Uuid) -> RepoResult<()>;
    /// Propose a remediation; returns `true` if newly created (deduped per
    /// `(rule, finding_key)`).
    async fn propose(&self, rule_id: Uuid, application_id: Uuid, finding_key: &str) -> RepoResult<bool>;
    async fn list_remediations(&self, status: Option<String>) -> RepoResult<Vec<Remediation>>;
    async fn get_remediation(&self, id: Uuid) -> RepoResult<Option<Remediation>>;
    async fn set_status(&self, id: Uuid, status: &str, agent_task_id: Option<Uuid>) -> RepoResult<()>;
}

macro_rules! remediation_impl {
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
        impl RemediationRepository for $name {
            async fn list_rules(&self) -> RepoResult<Vec<RemediationRule>> {
                Ok(sqlx::query_as(
                    "SELECT id, name, trigger_kind, params, action, prompt, scope, auto_approve, enabled \
                     FROM remediation_rules ORDER BY name",
                )
                .fetch_all(&self.pool)
                .await?)
            }

            async fn create_rule(&self, input: RuleInput) -> RepoResult<RemediationRule> {
                let id = Uuid::new_v4();
                sqlx::query(&$xform(
                    "INSERT INTO remediation_rules \
                       (id, name, trigger_kind, params, action, prompt, scope, auto_approve, enabled) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
                ))
                .bind(id)
                .bind(&input.name)
                .bind(&input.trigger_kind)
                .bind(&input.params)
                .bind(&input.action)
                .bind(&input.prompt)
                .bind(&input.scope)
                .bind(input.auto_approve)
                .bind(true)
                .execute(&self.pool)
                .await?;
                Ok(RemediationRule {
                    id,
                    name: input.name,
                    trigger_kind: input.trigger_kind,
                    params: input.params,
                    action: input.action,
                    prompt: input.prompt,
                    scope: input.scope,
                    auto_approve: input.auto_approve,
                    enabled: true,
                })
            }

            async fn delete_rule(&self, id: Uuid) -> RepoResult<()> {
                sqlx::query(&$xform("DELETE FROM remediation_rules WHERE id=$1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }

            async fn propose(&self, rule_id: Uuid, application_id: Uuid, finding_key: &str) -> RepoResult<bool> {
                let res = sqlx::query(&$xform(
                    "INSERT INTO remediations (id, rule_id, application_id, finding_key, status) \
                     VALUES ($1,$2,$3,$4,'proposed') ON CONFLICT (rule_id, finding_key) DO NOTHING",
                ))
                .bind(Uuid::new_v4())
                .bind(rule_id)
                .bind(application_id)
                .bind(finding_key)
                .execute(&self.pool)
                .await?;
                Ok(res.rows_affected() > 0)
            }

            async fn list_remediations(&self, status: Option<String>) -> RepoResult<Vec<Remediation>> {
                let cols = "id, rule_id, application_id, finding_key, status, agent_task_id, created_at";
                let rows: Vec<Remediation> = match status {
                    Some(s) => {
                        sqlx::query_as(&$xform(&format!(
                            "SELECT {cols} FROM remediations WHERE status=$1 ORDER BY created_at DESC"
                        )))
                        .bind(s)
                        .fetch_all(&self.pool)
                        .await?
                    }
                    None => {
                        sqlx::query_as(&format!("SELECT {cols} FROM remediations ORDER BY created_at DESC"))
                            .fetch_all(&self.pool)
                            .await?
                    }
                };
                Ok(rows)
            }

            async fn get_remediation(&self, id: Uuid) -> RepoResult<Option<Remediation>> {
                Ok(sqlx::query_as(&$xform(
                    "SELECT id, rule_id, application_id, finding_key, status, agent_task_id, created_at \
                     FROM remediations WHERE id=$1",
                ))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?)
            }

            async fn set_status(&self, id: Uuid, status: &str, agent_task_id: Option<Uuid>) -> RepoResult<()> {
                sqlx::query(&$xform("UPDATE remediations SET status=$2, agent_task_id=$3 WHERE id=$1"))
                    .bind(id)
                    .bind(status)
                    .bind(agent_task_id)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }
        }
    };
}

remediation_impl!(PgRemediationRepository, PgPool, identity);
remediation_impl!(SqliteRemediationRepository, SqlitePool, to_sqlite);
