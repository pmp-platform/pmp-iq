//! Dual-engine storage + nearest-neighbour search for entity embeddings (M40).
//! Vectors are stored as f32 blobs; `nearest` does a bounded cosine scan in
//! Rust (the catalog is small), so the same code path serves both engines.

use super::model::{EntityEmbedding, Neighbour, cosine, from_blob, rank, to_blob};
use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use sqlx::{PgPool, SqlitePool};
use std::collections::HashMap;
use uuid::Uuid;

/// Store + query catalog embeddings.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait EmbeddingRepository: Send + Sync {
    /// Insert or replace one entity's embedding for a model.
    async fn upsert(&self, model: &str, embedding: &EntityEmbedding) -> RepoResult<()>;
    /// `(entity_type, entity_id) → summary_hash` for a model (skip unchanged).
    async fn hashes(&self, model: &str) -> RepoResult<HashMap<(String, Uuid), String>>;
    /// All embeddings for a model, optionally filtered to one entity type.
    async fn all(&self, model: &str, entity_type: Option<String>) -> RepoResult<Vec<EntityEmbedding>>;
    /// Top-`k` nearest neighbours to `query` (cosine), optionally type-filtered.
    async fn nearest(
        &self,
        model: &str,
        query: Vec<f32>,
        entity_type: Option<String>,
        k: usize,
    ) -> RepoResult<Vec<Neighbour>>;
}

#[derive(sqlx::FromRow)]
struct Row {
    entity_type: String,
    entity_id: Uuid,
    vector: Vec<u8>,
    summary_hash: String,
}

impl From<Row> for EntityEmbedding {
    fn from(r: Row) -> Self {
        EntityEmbedding {
            entity_type: r.entity_type,
            entity_id: r.entity_id,
            vector: from_blob(&r.vector),
            summary_hash: r.summary_hash,
        }
    }
}

macro_rules! embedding_impl {
    ($name:ident, $pool:ty, $xform:path, $now:literal) => {
        pub struct $name {
            pool: $pool,
        }
        impl $name {
            pub fn new(pool: $pool) -> Self {
                Self { pool }
            }
        }
        #[async_trait]
        impl EmbeddingRepository for $name {
            async fn upsert(&self, model: &str, embedding: &EntityEmbedding) -> RepoResult<()> {
                let blob = to_blob(&embedding.vector);
                sqlx::query(&$xform(concat!(
                    "INSERT INTO entity_embeddings \
                       (entity_type, entity_id, model, dim, vector, summary_hash) \
                     VALUES ($1,$2,$3,$4,$5,$6) \
                     ON CONFLICT (entity_type, entity_id, model) DO UPDATE SET \
                       dim = excluded.dim, vector = excluded.vector, \
                       summary_hash = excluded.summary_hash, updated_at = ",
                    $now
                )))
                .bind(&embedding.entity_type)
                .bind(embedding.entity_id)
                .bind(model)
                .bind(embedding.vector.len() as i32)
                .bind(blob)
                .bind(&embedding.summary_hash)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn hashes(&self, model: &str) -> RepoResult<HashMap<(String, Uuid), String>> {
                let rows: Vec<(String, Uuid, String)> = sqlx::query_as(&$xform(
                    "SELECT entity_type, entity_id, summary_hash FROM entity_embeddings WHERE model = $1",
                ))
                .bind(model)
                .fetch_all(&self.pool)
                .await?;
                Ok(rows.into_iter().map(|(t, id, h)| ((t, id), h)).collect())
            }

            async fn all(&self, model: &str, entity_type: Option<String>) -> RepoResult<Vec<EntityEmbedding>> {
                let rows: Vec<Row> = match entity_type.as_deref() {
                    Some(t) => {
                        sqlx::query_as(&$xform(
                            "SELECT entity_type, entity_id, vector, summary_hash FROM entity_embeddings \
                             WHERE model = $1 AND entity_type = $2",
                        ))
                        .bind(model)
                        .bind(t)
                        .fetch_all(&self.pool)
                        .await?
                    }
                    None => {
                        sqlx::query_as(&$xform(
                            "SELECT entity_type, entity_id, vector, summary_hash FROM entity_embeddings \
                             WHERE model = $1",
                        ))
                        .bind(model)
                        .fetch_all(&self.pool)
                        .await?
                    }
                };
                Ok(rows.into_iter().map(EntityEmbedding::from).collect())
            }

            async fn nearest(
                &self,
                model: &str,
                query: Vec<f32>,
                entity_type: Option<String>,
                k: usize,
            ) -> RepoResult<Vec<Neighbour>> {
                let candidates = self.all(model, entity_type).await?;
                Ok(rank(&query, &candidates, k))
            }
        }
    };
}

embedding_impl!(PgEmbeddingRepository, PgPool, identity, "now()");
embedding_impl!(SqliteEmbeddingRepository, SqlitePool, to_sqlite, "CURRENT_TIMESTAMP");

/// Nearest neighbours of a stored entity (excluding itself), for "similar to X".
pub fn neighbours_of(all: &[EntityEmbedding], entity_id: Uuid, k: usize) -> Vec<Neighbour> {
    let Some(target) = all.iter().find(|e| e.entity_id == entity_id) else {
        return vec![];
    };
    let mut scored: Vec<Neighbour> = all
        .iter()
        .filter(|e| e.entity_id != entity_id)
        .map(|e| Neighbour {
            entity_type: e.entity_type.clone(),
            entity_id: e.entity_id,
            score: cosine(&target.vector, &e.vector),
        })
        .collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}
