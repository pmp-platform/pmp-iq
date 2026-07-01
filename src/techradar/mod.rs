//! Version currency & tech radar (M45): how outdated/EOL each app's dependencies
//! are, and an adopt/trial/assess/hold radar over technologies.

pub mod currency;
pub mod repository;

pub use currency::{DepCurrency, DepInput, assess, currency_score, is_current, major_behind};
pub use repository::{
    PgTechRadarRepository, PolicyInput, RadarEntry, RadarInput, SqliteTechRadarRepository,
    TechRadarRepository, VersionPolicy,
};

/// A small seed of common ecosystems/runtimes (operators update these).
pub fn default_policies() -> Vec<PolicyInput> {
    let p = |ecosystem: &str, name: &str, latest: &str, eol: Option<&str>| PolicyInput {
        ecosystem: ecosystem.into(),
        name: name.into(),
        latest: Some(latest.into()),
        eol_date: eol.map(String::from),
    };
    vec![
        p("", "rust", "1.85", None),
        p("", "python", "3.13", None),
        p("", "node", "22", Some("2025-04-30")),
        p("", "go", "1.24", None),
        p("cargo", "axum", "0.8", None),
        p("cargo", "tokio", "1", None),
        p("npm", "react", "19", None),
        p("pip", "django", "5", None),
    ]
}

/// Seed the default version policy into an empty table (boot-time).
pub async fn ensure_default_policies(repo: &dyn TechRadarRepository) -> crate::db::RepoResult<()> {
    if repo.count_policies().await? == 0 {
        for policy in default_policies() {
            repo.upsert_policy(policy).await?;
        }
    }
    Ok(())
}
