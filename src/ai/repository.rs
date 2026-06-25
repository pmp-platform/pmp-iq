//! Data access for AI agent profiles.

use super::model::{AiProfile, AiProfileInput, AiProviderType};
use crate::db::{RepoError, RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{FromRow, PgPool, SqlitePool};
use uuid::Uuid;

/// CRUD access to AI agent profiles.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait AiProfileRepository: Send + Sync {
    async fn create(&self, input: AiProfileInput) -> RepoResult<AiProfile>;
    async fn update(&self, id: Uuid, input: AiProfileInput) -> RepoResult<AiProfile>;
    async fn delete(&self, id: Uuid) -> RepoResult<()>;
    async fn get(&self, id: Uuid) -> RepoResult<AiProfile>;
    async fn list(&self) -> RepoResult<Vec<AiProfile>>;
}

#[derive(FromRow)]
struct ProfileRow {
    id: Uuid,
    name: String,
    provider_type: String,
    config: Value,
    secrets_enc: Option<Vec<u8>>,
    enabled: bool,
}

impl TryFrom<ProfileRow> for AiProfile {
    type Error = RepoError;

    fn try_from(row: ProfileRow) -> Result<Self, Self::Error> {
        Ok(AiProfile {
            id: row.id,
            name: row.name,
            provider_type: AiProviderType::parse(&row.provider_type).map_err(RepoError::Mapping)?,
            config: row.config,
            secrets_enc: row.secrets_enc,
            enabled: row.enabled,
        })
    }
}

const COLS: &str = "id, name, provider_type, config, secrets_enc, enabled";

macro_rules! ai_profile_repo_impl {
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
        impl AiProfileRepository for $name {
            async fn create(&self, input: AiProfileInput) -> RepoResult<AiProfile> {
                let id = Uuid::new_v4();
                let row: ProfileRow = sqlx::query_as(&$xform(
                    "INSERT INTO ai_agent_profiles (id, name, provider_type, config, secrets_enc, enabled) \
                     VALUES ($1,$2,$3,$4,$5,$6) \
                     RETURNING id, name, provider_type, config, secrets_enc, enabled",
                ))
                .bind(id)
                .bind(&input.name)
                .bind(input.provider_type.as_str())
                .bind(&input.config)
                .bind(&input.secrets_enc)
                .bind(input.enabled)
                .fetch_one(&self.pool)
                .await?;
                row.try_into()
            }

            async fn update(&self, id: Uuid, input: AiProfileInput) -> RepoResult<AiProfile> {
                let row: ProfileRow = sqlx::query_as(&$xform(
                    "UPDATE ai_agent_profiles SET name=$2, provider_type=$3, config=$4, \
                     secrets_enc=$5, enabled=$6, updated_at=CURRENT_TIMESTAMP WHERE id=$1 \
                     RETURNING id, name, provider_type, config, secrets_enc, enabled",
                ))
                .bind(id)
                .bind(&input.name)
                .bind(input.provider_type.as_str())
                .bind(&input.config)
                .bind(&input.secrets_enc)
                .bind(input.enabled)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;
                row.try_into()
            }

            async fn delete(&self, id: Uuid) -> RepoResult<()> {
                let res = sqlx::query(&$xform("DELETE FROM ai_agent_profiles WHERE id=$1"))
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                if res.rows_affected() == 0 {
                    return Err(RepoError::NotFound);
                }
                Ok(())
            }

            async fn get(&self, id: Uuid) -> RepoResult<AiProfile> {
                let row: ProfileRow = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM ai_agent_profiles WHERE id=$1"
                )))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(RepoError::NotFound)?;
                row.try_into()
            }

            async fn list(&self) -> RepoResult<Vec<AiProfile>> {
                let rows: Vec<ProfileRow> = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM ai_agent_profiles ORDER BY name"
                )))
                .fetch_all(&self.pool)
                .await?;
                rows.into_iter().map(AiProfile::try_from).collect()
            }
        }
    };
}

ai_profile_repo_impl!(PgAiProfileRepository, PgPool, identity);
ai_profile_repo_impl!(SqliteAiProfileRepository, SqlitePool, to_sqlite);
