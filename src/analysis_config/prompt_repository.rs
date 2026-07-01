//! Dual-engine persistence for extraction-prompt overrides (M34). Only sections
//! an operator has edited are stored; everything else uses the code defaults.

use crate::db::{RepoResult, identity, to_sqlite};
use async_trait::async_trait;
use sqlx::{PgPool, SqlitePool};

/// A stored override for one prompt section.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StoredPrompt {
    pub section_key: String,
    pub template: String,
    pub enabled: bool,
}

/// Read + write extraction-prompt overrides.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ExtractionPromptRepository: Send + Sync {
    /// All stored overrides (sections never edited are absent).
    async fn list(&self) -> RepoResult<Vec<StoredPrompt>>;
    /// Upsert one section's template + enabled flag.
    async fn upsert(&self, section: &str, template: &str, enabled: bool) -> RepoResult<()>;
    /// Remove a section's override, restoring its code default.
    async fn delete(&self, section: &str) -> RepoResult<()>;
}

macro_rules! prompt_impl {
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
        impl ExtractionPromptRepository for $name {
            async fn list(&self) -> RepoResult<Vec<StoredPrompt>> {
                let rows: Vec<StoredPrompt> = sqlx::query_as(
                    "SELECT section_key, template, enabled FROM extraction_prompts ORDER BY section_key",
                )
                .fetch_all(&self.pool)
                .await?;
                Ok(rows)
            }

            async fn upsert(&self, section: &str, template: &str, enabled: bool) -> RepoResult<()> {
                sqlx::query(&$xform(concat!(
                    "INSERT INTO extraction_prompts (section_key, template, enabled) VALUES ($1,$2,$3) \
                     ON CONFLICT (section_key) DO UPDATE SET template = excluded.template, \
                     enabled = excluded.enabled, updated_at = ",
                    $now
                )))
                .bind(section)
                .bind(template)
                .bind(enabled)
                .execute(&self.pool)
                .await?;
                Ok(())
            }

            async fn delete(&self, section: &str) -> RepoResult<()> {
                sqlx::query(&$xform("DELETE FROM extraction_prompts WHERE section_key = $1"))
                    .bind(section)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }
        }
    };
}

prompt_impl!(PgExtractionPromptRepository, PgPool, identity, "now()");
prompt_impl!(SqliteExtractionPromptRepository, SqlitePool, to_sqlite, "CURRENT_TIMESTAMP");
