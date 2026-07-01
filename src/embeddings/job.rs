//! The `generate-embeddings` job (M40): embeds catalog entities whose summary
//! changed since the last run (skipping unchanged ones via `summary_hash`).

use super::model::{EmbeddingSource, EntityEmbedding, build_summary, summary_hash};
use super::provider::EmbeddingProvider;
use super::repository::EmbeddingRepository;
use crate::db::RepoResult;
use crate::error::AppError;
use crate::jobs::model::{JobInput, TriggerType};
use crate::jobs::repository::JobRepository;
use crate::jobs::{JobContext, JobError, JobOutcome, JobType};
use crate::platform::PlatformQuery;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub const JOB_TYPE: &str = "generate-embeddings";

/// Supplies the entities to embed (decoupled from `PlatformQuery` for testing).
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait EmbeddingSourceQuery: Send + Sync {
    async fn sources(&self) -> RepoResult<Vec<EmbeddingSource>>;
}

/// Adapts a [`PlatformQuery`] into compact embedding summaries.
pub struct PlatformEmbeddingSources {
    platform: Arc<dyn PlatformQuery>,
}

impl PlatformEmbeddingSources {
    pub fn new(platform: Arc<dyn PlatformQuery>) -> Self {
        Self { platform }
    }
}

#[async_trait]
impl EmbeddingSourceQuery for PlatformEmbeddingSources {
    async fn sources(&self) -> RepoResult<Vec<EmbeddingSource>> {
        let rows = self.platform.embedding_sources().await?;
        Ok(rows
            .into_iter()
            .map(|r| EmbeddingSource {
                entity_type: r.entity_type,
                entity_id: r.entity_id,
                summary: build_summary(&r.name, &r.kind, &r.description),
            })
            .collect())
    }
}

/// Dependencies for the job (bundled to bound parameter count).
#[derive(Clone)]
pub struct GenerateEmbeddingsDeps {
    pub sources: Arc<dyn EmbeddingSourceQuery>,
    pub repo: Arc<dyn EmbeddingRepository>,
    pub provider: Arc<dyn EmbeddingProvider>,
}

pub struct GenerateEmbeddingsJob {
    deps: GenerateEmbeddingsDeps,
}

impl GenerateEmbeddingsJob {
    pub fn new(deps: GenerateEmbeddingsDeps) -> Self {
        Self { deps }
    }

    /// Embed only changed/new entities. Returns `(embedded, skipped)`.
    async fn generate(&self, ctx: &JobContext) -> Result<(usize, usize), JobError> {
        let model = self.deps.provider.model();
        let sources = self.deps.sources.sources().await.map_err(fail)?;
        let existing = self.deps.repo.hashes(&model).await.map_err(fail)?;

        let mut changed: Vec<(EmbeddingSource, String)> = Vec::new();
        let mut skipped = 0;
        for s in sources {
            let hash = summary_hash(&s.summary);
            let key = (s.entity_type.clone(), s.entity_id);
            if existing.get(&key).is_some_and(|h| *h == hash) {
                skipped += 1;
            } else {
                changed.push((s, hash));
            }
        }
        if changed.is_empty() {
            ctx.log(&format!("nothing to embed ({skipped} unchanged)")).await;
            return Ok((0, skipped));
        }

        let texts: Vec<String> = changed.iter().map(|(s, _)| s.summary.clone()).collect();
        let vectors = self.deps.provider.embed(&texts).await.map_err(|e| JobError::Failed(e.to_string()))?;
        if vectors.len() != changed.len() {
            return Err(JobError::Failed(format!(
                "provider returned {} vectors for {} inputs",
                vectors.len(),
                changed.len()
            )));
        }

        let mut embedded = 0;
        for ((source, hash), vector) in changed.into_iter().zip(vectors) {
            let embedding = EntityEmbedding {
                entity_type: source.entity_type,
                entity_id: source.entity_id,
                vector,
                summary_hash: hash,
            };
            self.deps.repo.upsert(&model, &embedding).await.map_err(fail)?;
            embedded += 1;
        }
        ctx.log(&format!("embedded {embedded}, skipped {skipped} unchanged")).await;
        Ok((embedded, skipped))
    }
}

fn fail(e: impl std::fmt::Display) -> JobError {
    JobError::Failed(e.to_string())
}

#[async_trait]
impl JobType for GenerateEmbeddingsJob {
    fn id(&self) -> &str {
        JOB_TYPE
    }

    fn description(&self) -> &str {
        "Generate semantic embeddings for catalog entities (semantic search & duplicates)"
    }

    async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError> {
        let (embedded, skipped) = self.generate(&ctx).await?;
        Ok(JobOutcome::completed(json!({ "embedded": embedded, "skipped": skipped })))
    }
}

/// Seed the singleton `generate-embeddings` job at boot (manual trigger).
pub async fn ensure_embeddings_job(jobs: &dyn JobRepository) -> Result<Uuid, AppError> {
    let existing = jobs.list().await?;
    if let Some(job) = existing.iter().find(|j| j.job_type == JOB_TYPE) {
        return Ok(job.id);
    }
    let created = jobs
        .create(JobInput {
            job_type: JOB_TYPE.to_string(),
            name: "Generate embeddings".to_string(),
            trigger_type: TriggerType::Manual,
            cron_expr: None,
            config: json!({}),
            enabled: true,
            next_run_at: None,
        })
        .await?;
    Ok(created.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embeddings::repository::MockEmbeddingRepository;
    use crate::embeddings::provider::MockEmbeddingProvider;
    use crate::jobs::clock::MockClock;
    use crate::jobs::log_sink::MockLogSink;
    use crate::jobs::repository::MockJobExecutionRepository;
    use serde_json::Value;
    use std::collections::HashMap;

    fn ctx() -> JobContext {
        let mut log = MockLogSink::new();
        log.expect_append().returning(|_, _| Ok(()));
        let mut clock = MockClock::new();
        clock.expect_now().returning(chrono::Utc::now);
        JobContext {
            execution_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            job_name: "embed".into(),
            config: Value::Null,
            params: Value::Null,
            state: Value::Null,
            log: Arc::new(log),
            executions: Arc::new(MockJobExecutionRepository::new()),
            clock: Arc::new(clock),
        }
    }

    fn source(id: Uuid, summary: &str) -> EmbeddingSource {
        EmbeddingSource { entity_type: "application".into(), entity_id: id, summary: summary.into() }
    }

    #[tokio::test]
    async fn embeds_only_changed_entities() {
        let unchanged = Uuid::new_v4();
        let changed = Uuid::new_v4();
        let unchanged_summary = "stable summary";

        let mut sources = MockEmbeddingSourceQuery::new();
        sources.expect_sources().returning(move || {
            Ok(vec![source(unchanged, unchanged_summary), source(changed, "new summary")])
        });

        let mut provider = MockEmbeddingProvider::new();
        provider.expect_model().return_const("test-model".to_string());
        // Only the changed entity's summary is sent to the provider.
        provider
            .expect_embed()
            .withf(|texts: &[String]| texts == ["new summary".to_string()])
            .returning(|_| Ok(vec![vec![0.1, 0.2]]));

        let mut repo = MockEmbeddingRepository::new();
        repo.expect_hashes().returning(move |_| {
            let mut m = HashMap::new();
            m.insert(("application".to_string(), unchanged), summary_hash(unchanged_summary));
            Ok(m)
        });
        repo.expect_upsert()
            .withf(move |_, e| e.entity_id == changed)
            .times(1)
            .returning(|_, _| Ok(()));

        let job = GenerateEmbeddingsJob::new(GenerateEmbeddingsDeps {
            sources: Arc::new(sources),
            repo: Arc::new(repo),
            provider: Arc::new(provider),
        });
        let (embedded, skipped) = job.generate(&ctx()).await.unwrap();
        assert_eq!(embedded, 1);
        assert_eq!(skipped, 1);
    }

    #[tokio::test]
    async fn no_changes_skips_provider() {
        let id = Uuid::new_v4();
        let mut sources = MockEmbeddingSourceQuery::new();
        sources.expect_sources().returning(move || Ok(vec![source(id, "same")]));
        let mut provider = MockEmbeddingProvider::new();
        provider.expect_model().return_const("m".to_string());
        // embed() must not be called.
        let mut repo = MockEmbeddingRepository::new();
        repo.expect_hashes().returning(move |_| {
            Ok(HashMap::from([(("application".to_string(), id), summary_hash("same"))]))
        });

        let job = GenerateEmbeddingsJob::new(GenerateEmbeddingsDeps {
            sources: Arc::new(sources),
            repo: Arc::new(repo),
            provider: Arc::new(provider),
        });
        let (embedded, skipped) = job.generate(&ctx()).await.unwrap();
        assert_eq!(embedded, 0);
        assert_eq!(skipped, 1);
    }
}
