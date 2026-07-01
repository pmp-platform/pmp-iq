//! Semantic search & duplicate detection (M40): embed catalog entities, then
//! search by meaning, find similar entities, and cluster likely duplicates.

pub mod job;
pub mod model;
pub mod provider;
pub mod repository;

pub use job::{
    EmbeddingSourceQuery, GenerateEmbeddingsDeps, GenerateEmbeddingsJob, JOB_TYPE,
    PlatformEmbeddingSources, ensure_embeddings_job,
};
pub use model::{
    EmbeddingSource, EntityEmbedding, Neighbour, build_summary, cluster, cosine, rank, summary_hash,
};
pub use provider::{EmbeddingConfig, EmbeddingError, EmbeddingProvider, HttpEmbeddingProvider};
pub use repository::{
    EmbeddingRepository, PgEmbeddingRepository, SqliteEmbeddingRepository, neighbours_of,
};
