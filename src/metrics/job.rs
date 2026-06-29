//! The `collect-metrics` job (M31): clone an application's repository and have
//! the LLM extract quality metrics (tests, coverage, complexity, LOC) from the
//! checkout's CI logs / coverage reports / code, normalised into a uniform set.

use super::model::Metric;
use super::repository::ApplicationMetricsRepository;
use crate::accounts::AccountService;
use crate::ai::{AiProfileService, AiProvider, AiProviderDeps, AiProviderFactory, AiRequest};
use crate::error::AppError;
use crate::git::{CloneRequest, GitClient};
use crate::jobs::model::{JobInput, TriggerType};
use crate::jobs::repository::JobRepository;
use crate::jobs::{JobContext, JobError, JobOutcome, JobType, RecordingAiProvider};
use crate::locks::{DistributedLock, lock_keys};
use crate::platform::query::PlatformQuery;
use crate::repositories::{RepoRecord, RepoRecordRepository};
use crate::workspace::{JobLocator, Workspace};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub const JOB_TYPE: &str = "collect-metrics";
const LOCK_TTL: Duration = Duration::from_secs(900);

const SYSTEM: &str = "You are a software metrics analyst. Inspect the repository in your working \
    directory — its CI configuration and logs, coverage/test reports, and source code.";

const PROMPT: &str = "Determine these metrics and output ONLY a single JSON object with these keys \
    (use null for anything the evidence does not support — never guess):\n\
    { \"tests_total\": int, \"tests_passed\": int, \"tests_failed\": int, \"coverage_pct\": number, \
      \"complexity_avg\": number, \"loc\": int, \"has_ci\": boolean }";

/// Dependencies (bundled to bound parameter count).
#[derive(Clone)]
pub struct CollectMetricsDeps {
    pub platform: Arc<dyn PlatformQuery>,
    pub repositories: Arc<dyn RepoRecordRepository>,
    pub accounts: AccountService,
    pub git: Arc<dyn GitClient>,
    pub workspace: Workspace,
    pub ai: AiProfileService,
    pub ai_deps: AiProviderDeps,
    pub metrics: Arc<dyn ApplicationMetricsRepository>,
    pub lock: Arc<dyn DistributedLock>,
}

#[derive(Deserialize)]
struct Input {
    application_id: Uuid,
    ai_profile_id: Uuid,
}

#[derive(Deserialize, Default)]
struct RawMetrics {
    tests_total: Option<f64>,
    tests_passed: Option<f64>,
    tests_failed: Option<f64>,
    coverage_pct: Option<f64>,
    complexity_avg: Option<f64>,
    loc: Option<f64>,
    has_ci: Option<bool>,
}

pub struct CollectMetricsJob {
    deps: CollectMetricsDeps,
}

impl CollectMetricsJob {
    pub fn new(deps: CollectMetricsDeps) -> Self {
        Self { deps }
    }

    fn parse_input(ctx: &JobContext) -> Result<Input, JobError> {
        serde_json::from_value(ctx.params.clone())
            .map_err(|e| JobError::Failed(format!("invalid collect-metrics params: {e}")))
    }

    async fn checkout(&self, ctx: &JobContext, record: &RepoRecord) -> Result<String, JobError> {
        let account = self
            .deps
            .accounts
            .get(record.account_id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let token = self
            .deps
            .accounts
            .clone_token(&account)
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let dest = self
            .deps
            .workspace
            .repo_dir(&JobLocator::new(&ctx.job_name, ctx.job_id), &record.full_name)
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let branch = record.default_branch.clone().or_else(|| Some("main".to_string()));
        self.deps
            .git
            .sync_branch(CloneRequest { clone_url: record.clone_url.clone(), dest: dest.clone(), branch, token })
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        Ok(dest)
    }

    async fn provider(&self, ctx: &JobContext, profile_id: Uuid) -> Result<RecordingAiProvider, JobError> {
        let profile = self
            .deps
            .ai
            .get(profile_id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let inner = AiProviderFactory::build(&profile, &self.deps.ai_deps)
            .map_err(|e| JobError::Failed(e.to_string()))?;
        Ok(ctx.recording_provider(inner))
    }

    async fn collect(&self, ctx: &JobContext, app_id: Uuid, record: &RepoRecord, profile_id: Uuid) -> Result<JobOutcome, JobError> {
        let checkout = self.checkout(ctx, record).await?;
        let provider = self.provider(ctx, profile_id).await?;
        let request = AiRequest::new(PROMPT)
            .with_system(SYSTEM.to_string())
            .with_working_dir(checkout);
        let response = provider
            .complete(request)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let metrics = parse_metrics(&response.text);
        self.deps
            .metrics
            .record(app_id, "llm", &metrics)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        Ok(JobOutcome::completed(json!({
            "application_id": app_id,
            "collected": metrics.len(),
        })))
    }
}

#[async_trait]
impl JobType for CollectMetricsJob {
    fn id(&self) -> &str {
        JOB_TYPE
    }

    fn description(&self) -> &str {
        "Clone an application's repository and have the LLM extract uniform quality metrics \
         (tests, coverage, complexity, LOC) from its CI and codebase."
    }

    async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError> {
        let input = Self::parse_input(&ctx)?;
        let repo_id = self
            .deps
            .platform
            .application_repository(input.application_id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?
            .ok_or_else(|| JobError::Failed("application has no linked repository".into()))?;
        let record = self
            .deps
            .repositories
            .get(repo_id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;

        let key = lock_keys::repository(&record.full_name);
        let lease = match self.deps.lock.acquire(&key, LOCK_TTL).await {
            Ok(Some(lease)) => lease,
            Ok(None) => {
                ctx.log("repository is busy — rescheduling").await;
                return Err(JobError::CannotRun { retry_at: None });
            }
            Err(e) => return Err(JobError::Failed(format!("repository lock error: {e}"))),
        };
        let outcome = self.collect(&ctx, input.application_id, &record, input.ai_profile_id).await;
        let _ = self.deps.lock.release(&lease).await;
        outcome
    }
}

/// Extract the first JSON object in `text` and normalise it into metrics.
/// Absent fields are omitted (never fabricated).
fn parse_metrics(text: &str) -> Vec<Metric> {
    let raw: RawMetrics = serde_json::from_str(&extract_json(text)).unwrap_or_default();
    let mut out = Vec::new();
    let mut push = |key: &str, v: Option<f64>, unit: Option<&str>| {
        if let Some(v) = v {
            out.push(Metric::new(key, v, unit));
        }
    };
    push("tests_total", raw.tests_total, Some("count"));
    push("tests_passed", raw.tests_passed, Some("count"));
    push("tests_failed", raw.tests_failed, Some("count"));
    push("coverage_pct", raw.coverage_pct, Some("percent"));
    push("complexity_avg", raw.complexity_avg, None);
    push("loc", raw.loc, Some("count"));
    if let Some(b) = raw.has_ci {
        out.push(Metric::new("has_ci", if b { 1.0 } else { 0.0 }, Some("bool")));
    }
    out
}

fn extract_json(text: &str) -> String {
    match (text.find('{'), text.rfind('}')) {
        (Some(s), Some(e)) if e > s => text[s..=e].to_string(),
        _ => "{}".to_string(),
    }
}

/// Find the singleton `collect-metrics` job, creating it if absent.
pub async fn ensure_job(jobs: &dyn JobRepository) -> Result<Uuid, AppError> {
    let existing = jobs.list().await?;
    if let Some(job) = existing.iter().find(|j| j.job_type == JOB_TYPE) {
        return Ok(job.id);
    }
    let created = jobs
        .create(JobInput {
            job_type: JOB_TYPE.to_string(),
            name: "Collect metrics".to_string(),
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

    #[test]
    fn parses_metrics_and_omits_nulls() {
        let text = "Here are the metrics:\n{\"tests_total\":120,\"tests_passed\":118,\"tests_failed\":2,\
            \"coverage_pct\":83.5,\"complexity_avg\":null,\"loc\":21450,\"has_ci\":true} done";
        let m = parse_metrics(text);
        let by = |k: &str| m.iter().find(|x| x.key == k).map(|x| x.value);
        assert_eq!(by("tests_total"), Some(120.0));
        assert_eq!(by("coverage_pct"), Some(83.5));
        assert_eq!(by("loc"), Some(21450.0));
        assert_eq!(by("has_ci"), Some(1.0));
        assert!(by("complexity_avg").is_none(), "null is omitted");
    }

    #[test]
    fn parse_metrics_handles_no_json() {
        assert!(parse_metrics("no json here").is_empty());
    }
}
