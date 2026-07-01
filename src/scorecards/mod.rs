//! Production-readiness scorecards (M43): configurable checks evaluated against
//! the model + metrics + ownership into a weighted score and maturity level.

pub mod engine;
pub mod repository;

pub use engine::{
    Check, CheckResult, Scorecard, ScorecardInput, default_checks, evaluate, level_for,
};
pub use repository::{PgScorecardRepository, ScorecardRepository, SqliteScorecardRepository};

/// Seed the default checks into an empty `scorecard_checks` table (boot-time).
pub async fn ensure_default_checks(repo: &dyn ScorecardRepository) -> crate::db::RepoResult<()> {
    if repo.count_checks().await? == 0 {
        for check in default_checks() {
            repo.upsert_check(&check).await?;
        }
    }
    Ok(())
}
