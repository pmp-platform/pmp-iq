//! Dual-engine persistence for XP awards + badges (M44).

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, SqlitePool};
use uuid::Uuid;

/// An XP award to record (idempotent by `(actor, reason, source)`).
#[derive(Debug, Clone)]
pub struct XpAwardInput {
    pub actor: String,
    pub reason: String,
    pub points: i32,
    pub skill: Option<String>,
    pub source: Option<String>,
}

/// A recorded award (for the profile feed).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct XpAward {
    pub reason: String,
    pub points: i32,
    pub skill: Option<String>,
    pub awarded_at: DateTime<Utc>,
}

/// A leaderboard row.
#[derive(Debug, Clone, Serialize)]
pub struct ActorTotal {
    pub actor: String,
    pub points: i64,
    pub awards: i64,
}

/// Record XP awards + badges and read them back.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait GamificationRepository: Send + Sync {
    /// Insert an award; returns `true` if newly recorded (idempotent).
    async fn record(&self, award: XpAwardInput) -> RepoResult<bool>;
    /// Total XP + award count per actor, descending (leaderboard).
    async fn totals(&self) -> RepoResult<Vec<ActorTotal>>;
    async fn for_actor(&self, actor: &str) -> RepoResult<Vec<XpAward>>;
    /// XP per skill for one actor, descending.
    async fn skills_for(&self, actor: &str) -> RepoResult<Vec<(String, i64)>>;
    async fn set_badge(&self, actor: &str, badge: &str) -> RepoResult<()>;
    async fn badges_for(&self, actor: &str) -> RepoResult<Vec<String>>;
}

macro_rules! gamification_impl {
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
        impl GamificationRepository for $name {
            async fn record(&self, award: XpAwardInput) -> RepoResult<bool> {
                let res = sqlx::query(&$xform(
                    "INSERT INTO xp_awards (id, actor, reason, points, skill, source) \
                     VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (actor, reason, source) DO NOTHING",
                ))
                .bind(Uuid::new_v4())
                .bind(&award.actor)
                .bind(&award.reason)
                .bind(award.points)
                .bind(&award.skill)
                .bind(&award.source)
                .execute(&self.pool)
                .await?;
                Ok(res.rows_affected() > 0)
            }

            async fn totals(&self) -> RepoResult<Vec<ActorTotal>> {
                let rows: Vec<(String, i64, i64)> = sqlx::query_as(
                    "SELECT actor, CAST(COALESCE(SUM(points),0) AS BIGINT), CAST(COUNT(*) AS BIGINT) \
                     FROM xp_awards GROUP BY actor ORDER BY 2 DESC",
                )
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(|(actor, points, awards)| ActorTotal { actor, points, awards }).collect())
            }

            async fn for_actor(&self, actor: &str) -> RepoResult<Vec<XpAward>> {
                let rows: Vec<XpAward> = sqlx::query_as(&$xform(
                    "SELECT reason, points, skill, awarded_at FROM xp_awards WHERE actor=$1 ORDER BY awarded_at DESC",
                ))
                .bind(actor)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows)
            }

            async fn skills_for(&self, actor: &str) -> RepoResult<Vec<(String, i64)>> {
                let rows: Vec<(String, i64)> = sqlx::query_as(&$xform(
                    "SELECT skill, CAST(COALESCE(SUM(points),0) AS BIGINT) FROM xp_awards \
                     WHERE actor=$1 AND skill IS NOT NULL GROUP BY skill ORDER BY 2 DESC",
                ))
                .bind(actor)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows)
            }

            async fn set_badge(&self, actor: &str, badge: &str) -> RepoResult<()> {
                sqlx::query(&$xform(
                    "INSERT INTO badges (actor, badge) VALUES ($1,$2) ON CONFLICT DO NOTHING",
                ))
                .bind(actor)
                .bind(badge)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn badges_for(&self, actor: &str) -> RepoResult<Vec<String>> {
                let rows: Vec<(String,)> =
                    sqlx::query_as(&$xform("SELECT badge FROM badges WHERE actor=$1 ORDER BY badge"))
                        .bind(actor)
                        .fetch_all(&self.pool)
                        .await?;
                Ok(rows.into_iter().map(|(b,)| b).collect())
            }
        }
    };
}

gamification_impl!(PgGamificationRepository, PgPool, identity);
gamification_impl!(SqliteGamificationRepository, SqlitePool, to_sqlite);
