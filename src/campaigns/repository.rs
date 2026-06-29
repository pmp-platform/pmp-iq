//! Dual-engine persistence for batch-change campaigns.

use super::model::{Campaign, NewCampaign};
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, SqlitePool};
use uuid::Uuid;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CampaignRepository: Send + Sync {
    async fn create(&self, input: NewCampaign) -> RepoResult<Campaign>;
    async fn get(&self, id: Uuid) -> RepoResult<Campaign>;
    async fn list(&self) -> RepoResult<Vec<Campaign>>;
}

#[derive(FromRow)]
struct Row {
    id: Uuid,
    name: String,
    instruction: String,
    selection: String,
    task_id: Uuid,
    status: String,
    created_at: DateTime<Utc>,
}

impl From<Row> for Campaign {
    fn from(r: Row) -> Self {
        Campaign {
            id: r.id,
            name: r.name,
            instruction: r.instruction,
            selection: r.selection,
            task_id: r.task_id,
            status: r.status,
            created_at: r.created_at,
        }
    }
}

const COLS: &str = "id, name, instruction, selection, task_id, status, created_at";

macro_rules! campaign_impl {
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
        impl CampaignRepository for $name {
            async fn create(&self, input: NewCampaign) -> RepoResult<Campaign> {
                let row: Row = sqlx::query_as(&$xform(&format!(
                    "INSERT INTO campaigns (id, name, instruction, selection, task_id) \
                     VALUES ($1,$2,$3,$4,$5) RETURNING {COLS}"
                )))
                .bind(Uuid::new_v4())
                .bind(&input.name)
                .bind(&input.instruction)
                .bind(&input.selection)
                .bind(input.task_id)
                .fetch_one(&self.pool)
                .await?;
                Ok(row.into())
            }

            async fn get(&self, id: Uuid) -> RepoResult<Campaign> {
                let row: Row = sqlx::query_as(&$xform(&format!(
                    "SELECT {COLS} FROM campaigns WHERE id=$1"
                )))
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
                Ok(row.into())
            }

            async fn list(&self) -> RepoResult<Vec<Campaign>> {
                let rows: Vec<Row> = sqlx::query_as(&format!(
                    "SELECT {COLS} FROM campaigns ORDER BY created_at DESC"
                ))
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(Campaign::from).collect())
            }
        }
    };
}

campaign_impl!(PgCampaignRepository, PgPool, identity);
campaign_impl!(SqliteCampaignRepository, SqlitePool, to_sqlite);
