//! Data access for per-entity LLM hints.

use super::model::{EntityHint, EntityHintInput};
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use sqlx::{FromRow, PgPool, SqlitePool};
use uuid::Uuid;

/// CRUD access to entity hints.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait EntityHintRepository: Send + Sync {
    /// Insert or replace the hint for (application, entity_type, entity_key).
    async fn upsert(&self, input: EntityHintInput) -> RepoResult<EntityHint>;
    /// Remove a hint (no-op if absent).
    async fn delete(&self, application_id: Uuid, entity_type: &str, entity_key: &str) -> RepoResult<()>;
    /// All hints configured for an application.
    async fn list_for_application(&self, application_id: Uuid) -> RepoResult<Vec<EntityHint>>;
}

#[derive(FromRow)]
struct Row {
    id: Uuid,
    application_id: Uuid,
    entity_type: String,
    entity_key: String,
    hint: String,
}

impl From<Row> for EntityHint {
    fn from(row: Row) -> Self {
        EntityHint {
            id: row.id,
            application_id: row.application_id,
            entity_type: row.entity_type,
            entity_key: row.entity_key,
            hint: row.hint,
        }
    }
}

const COLS: &str = "id, application_id, entity_type, entity_key, hint";

macro_rules! entity_hint_impl {
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
        impl EntityHintRepository for $name {
            async fn upsert(&self, input: EntityHintInput) -> RepoResult<EntityHint> {
                let id = Uuid::new_v4();
                let row: Row = sqlx::query_as(&$xform(&format!(
                    "INSERT INTO entity_hints (id, application_id, entity_type, entity_key, hint) \
                     VALUES ($1,$2,$3,$4,$5) \
                     ON CONFLICT (application_id, entity_type, entity_key) DO UPDATE SET \
                       hint = EXCLUDED.hint, updated_at = CURRENT_TIMESTAMP \
                     RETURNING {COLS}",
                )))
                .bind(id)
                .bind(input.application_id)
                .bind(&input.entity_type)
                .bind(&input.entity_key)
                .bind(&input.hint)
                .fetch_one(&self.pool)
                .await?;
                Ok(row.into())
            }

            async fn delete(
                &self,
                application_id: Uuid,
                entity_type: &str,
                entity_key: &str,
            ) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "DELETE FROM entity_hints \
                     WHERE application_id=$1 AND entity_type=$2 AND entity_key=$3",
                ))
                .bind(application_id)
                .bind(entity_type)
                .bind(entity_key)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn list_for_application(
                &self,
                application_id: Uuid,
            ) -> RepoResult<Vec<EntityHint>> {
                let rows: Vec<Row> = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM entity_hints WHERE application_id=$1 \
                     ORDER BY entity_type, entity_key"
                )))
                .bind(application_id)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(EntityHint::from).collect())
            }
        }
    };
}

entity_hint_impl!(PgEntityHintRepository, PgPool, identity);
entity_hint_impl!(SqliteEntityHintRepository, SqlitePool, to_sqlite);
