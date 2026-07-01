//! Dual-engine persistence for the version policy + tech radar (M45).

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use serde::Serialize;
use sqlx::{PgPool, SqlitePool};
use uuid::Uuid;

/// Known-current version + EOL for a technology (`ecosystem` is '' for languages).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct VersionPolicy {
    pub ecosystem: String,
    pub name: String,
    pub latest: Option<String>,
    pub eol_date: Option<String>,
}

/// Fields to upsert a policy.
#[derive(Debug, Clone)]
pub struct PolicyInput {
    pub ecosystem: String,
    pub name: String,
    pub latest: Option<String>,
    pub eol_date: Option<String>,
}

/// A tech-radar placement.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RadarEntry {
    pub id: Uuid,
    pub quadrant: String,
    pub name: String,
    pub ring: String,
    pub note: Option<String>,
}

/// Fields to upsert a radar entry.
#[derive(Debug, Clone)]
pub struct RadarInput {
    pub quadrant: String,
    pub name: String,
    pub ring: String,
    pub note: Option<String>,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait TechRadarRepository: Send + Sync {
    async fn list_policies(&self) -> RepoResult<Vec<VersionPolicy>>;
    async fn upsert_policy(&self, input: PolicyInput) -> RepoResult<()>;
    async fn count_policies(&self) -> RepoResult<i64>;
    async fn list_radar(&self) -> RepoResult<Vec<RadarEntry>>;
    async fn upsert_radar(&self, input: RadarInput) -> RepoResult<()>;
    async fn delete_radar(&self, id: Uuid) -> RepoResult<()>;
}

macro_rules! techradar_impl {
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
        impl TechRadarRepository for $name {
            async fn list_policies(&self) -> RepoResult<Vec<VersionPolicy>> {
                Ok(sqlx::query_as("SELECT ecosystem, name, latest, eol_date FROM version_policy ORDER BY ecosystem, name")
                    .fetch_all(&self.pool)
                    .await?)
            }

            async fn upsert_policy(&self, input: PolicyInput) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "INSERT INTO version_policy (id, ecosystem, name, latest, eol_date) VALUES ($1,$2,$3,$4,$5) \
                     ON CONFLICT (ecosystem, name) DO UPDATE SET latest=excluded.latest, eol_date=excluded.eol_date",
                ))
                .bind(Uuid::new_v4())
                .bind(&input.ecosystem)
                .bind(&input.name)
                .bind(&input.latest)
                .bind(&input.eol_date)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn count_policies(&self) -> RepoResult<i64> {
                let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM version_policy").fetch_one(&self.pool).await?;
                Ok(n)
            }

            async fn list_radar(&self) -> RepoResult<Vec<RadarEntry>> {
                Ok(sqlx::query_as("SELECT id, quadrant, name, ring, note FROM tech_radar ORDER BY quadrant, name")
                    .fetch_all(&self.pool)
                    .await?)
            }

            async fn upsert_radar(&self, input: RadarInput) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "INSERT INTO tech_radar (id, quadrant, name, ring, note) VALUES ($1,$2,$3,$4,$5) \
                     ON CONFLICT (quadrant, name) DO UPDATE SET ring=excluded.ring, note=excluded.note",
                ))
                .bind(Uuid::new_v4())
                .bind(&input.quadrant)
                .bind(&input.name)
                .bind(&input.ring)
                .bind(&input.note)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn delete_radar(&self, id: Uuid) -> RepoResult<()> {
                sqlx::query(&$xform("DELETE FROM tech_radar WHERE id=$1")).bind(id).execute(&self.pool).await?;
                Ok(())
            }
        }
    };
}

techradar_impl!(PgTechRadarRepository, PgPool, identity);
techradar_impl!(SqliteTechRadarRepository, SqlitePool, to_sqlite);
