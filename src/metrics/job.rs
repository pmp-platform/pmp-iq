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
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub const JOB_TYPE: &str = "collect-metrics";
const LOCK_TTL: Duration = Duration::from_secs(900);

/// One numeric/bool field to pull out of an LLM pass's JSON response.
struct FieldSpec {
    key: &'static str,
    unit: Option<&'static str>,
    is_bool: bool,
}

const fn num(key: &'static str, unit: Option<&'static str>) -> FieldSpec {
    FieldSpec { key, unit, is_bool: false }
}

const fn flag(key: &'static str) -> FieldSpec {
    FieldSpec { key, unit: Some("bool"), is_bool: true }
}

/// One focused LLM extraction pass: a system+prompt and the fields it yields.
struct Pass {
    system: &'static str,
    prompt: &'static str,
    fields: &'static [FieldSpec],
}

const HEALTH_FIELDS: &[FieldSpec] = &[
    num("tests_total", Some("count")),
    num("tests_passed", Some("count")),
    num("tests_failed", Some("count")),
    num("coverage_pct", Some("percent")),
    num("complexity_avg", None),
    num("loc", Some("count")),
    flag("has_ci"),
    num("duplication_pct", Some("percent")),
    num("lint_warnings", Some("count")),
    num("todo_count", Some("count")),
    num("doc_coverage_pct", Some("percent")),
    num("fns_over_50_lines", Some("count")),
    num("files_over_1000_lines", Some("count")),
    num("fns_over_4_params", Some("count")),
];

const SECURITY_FIELDS: &[FieldSpec] = &[
    num("vuln_critical", Some("count")),
    num("vuln_high", Some("count")),
    num("vuln_medium", Some("count")),
    num("vuln_low", Some("count")),
    num("deps_outdated", Some("count")),
    num("dependency_count", Some("count")),
    num("secrets_detected", Some("count")),
    num("max_dep_age_days", Some("days")),
];

/// The LLM passes run per repository (each reads the checkout). Architecture and
/// model-coverage metrics are derived in Rust (see `derived_metrics`), not here.
const PASSES: &[Pass] = &[
    Pass {
        system: "You are a software metrics analyst. Inspect the repository in your working \
            directory — its CI configuration and logs, coverage/test reports, and source code.",
        prompt: "Determine these metrics and output ONLY a single JSON object with these keys \
            (use null for anything the evidence does not support — never guess):\n\
            { \"tests_total\": int, \"tests_passed\": int, \"tests_failed\": int, \
              \"coverage_pct\": number, \"complexity_avg\": number, \"loc\": int, \"has_ci\": boolean, \
              \"duplication_pct\": number, \"lint_warnings\": int, \"todo_count\": int, \
              \"doc_coverage_pct\": number, \"fns_over_50_lines\": int, \
              \"files_over_1000_lines\": int, \"fns_over_4_params\": int }",
        fields: HEALTH_FIELDS,
    },
    Pass {
        system: "You are a security & supply-chain analyst. Inspect the repository in your working \
            directory — its dependency manifests/lockfiles, known advisories, and source code.",
        prompt: "Assess security & supply chain and output ONLY a single JSON object with these keys \
            (use null for anything the evidence does not support — never guess):\n\
            { \"vuln_critical\": int, \"vuln_high\": int, \"vuln_medium\": int, \"vuln_low\": int, \
              \"deps_outdated\": int, \"dependency_count\": int, \"secrets_detected\": int, \
              \"max_dep_age_days\": int }",
        fields: SECURITY_FIELDS,
    },
];

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
        let llm = llm_metrics(&provider, &checkout).await?;
        self.record_set(app_id, "llm", &llm).await?;
        let derived = self.collect_derived(app_id).await?;
        self.record_set(app_id, "derived", &derived).await?;
        Ok(JobOutcome::completed(json!({
            "application_id": app_id,
            "collected": llm.len() + derived.len(),
        })))
    }

    /// Persist a metric set, mapping a store error into a job failure.
    async fn record_set(&self, app_id: Uuid, source: &str, metrics: &[Metric]) -> Result<(), JobError> {
        self.deps
            .metrics
            .record(app_id, source, metrics)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))
    }

    /// Metrics derived from the platform model for this app (no LLM).
    async fn collect_derived(&self, app_id: Uuid) -> Result<Vec<Metric>, JobError> {
        let detail = self
            .deps
            .platform
            .detail("applications", app_id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        Ok(derived_metrics(&detail))
    }
}

/// Run every LLM pass over the checkout and concatenate their metrics.
async fn llm_metrics(provider: &RecordingAiProvider, checkout: &str) -> Result<Vec<Metric>, JobError> {
    let mut out = Vec::new();

    for pass in PASSES {
        out.extend(run_pass(provider, checkout, pass).await?);
    }
    Ok(out)
}

/// Run one LLM pass and parse its configured fields.
async fn run_pass(provider: &RecordingAiProvider, checkout: &str, pass: &Pass) -> Result<Vec<Metric>, JobError> {
    let request = AiRequest::new(pass.prompt)
        .with_system(pass.system.to_string())
        .with_working_dir(checkout.to_string());
    let response = provider
        .complete(request)
        .await
        .map_err(|e| JobError::Failed(e.to_string()))?;
    Ok(parse_fields(&response.text, pass.fields))
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

/// Pull the configured numeric/bool fields out of the first JSON object in
/// `text`. Absent or null fields are omitted (never fabricated).
fn parse_fields(text: &str, fields: &[FieldSpec]) -> Vec<Metric> {
    let obj: Value = serde_json::from_str(&extract_json(text)).unwrap_or(Value::Null);
    let mut out = Vec::new();

    for f in fields {
        let raw = obj.get(f.key);
        let value = if f.is_bool {
            raw.and_then(Value::as_bool).map(|b| if b { 1.0 } else { 0.0 })
        } else {
            raw.and_then(Value::as_f64)
        };

        if let Some(value) = value {
            out.push(Metric::new(f.key, value, f.unit));
        }
    }
    out
}

/// Metrics derived from an application's platform-model detail (no LLM):
/// architecture (dependency fan-out / external count) and model coverage
/// (sub-entities present). Pure over the detail JSON, so it is unit-testable.
fn derived_metrics(detail: &Value) -> Vec<Metric> {
    let arr = |key: &str| detail.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let deps = arr("dependencies");
    let components = arr("components");
    let use_cases = arr("use_cases");
    let external = deps.iter().filter(|d| d.get("target_app_id").is_none_or(Value::is_null)).count();
    let signals: usize = components
        .iter()
        .map(|c| c.get("observability_signals").and_then(Value::as_array).map_or(0, |a| a.len()))
        .sum();
    let has_diagrams = use_cases
        .iter()
        .any(|u| u.get("diagrams").and_then(Value::as_array).is_some_and(|d| !d.is_empty()));
    let n = |count: usize| count as f64;

    vec![
        Metric::new("fan_out", n(deps.len()), Some("count")),
        Metric::new("external_dependency_count", n(external), Some("count")),
        Metric::new("component_count", n(components.len()), Some("count")),
        Metric::new("use_case_count", n(use_cases.len()), Some("count")),
        Metric::new("observability_signal_count", n(signals), Some("count")),
        Metric::new("has_use_cases", if use_cases.is_empty() { 0.0 } else { 1.0 }, Some("bool")),
        Metric::new("has_diagrams", if has_diagrams { 1.0 } else { 0.0 }, Some("bool")),
    ]
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

    fn by_key(m: &[Metric], k: &str) -> Option<f64> {
        m.iter().find(|x| x.key == k).map(|x| x.value)
    }

    #[test]
    fn parses_health_fields_and_omits_nulls() {
        let text = "Here are the metrics:\n{\"tests_total\":120,\"tests_passed\":118,\"tests_failed\":2,\
            \"coverage_pct\":83.5,\"complexity_avg\":null,\"loc\":21450,\"has_ci\":true,\
            \"todo_count\":7} done";
        let m = parse_fields(text, HEALTH_FIELDS);
        assert_eq!(by_key(&m, "tests_total"), Some(120.0));
        assert_eq!(by_key(&m, "coverage_pct"), Some(83.5));
        assert_eq!(by_key(&m, "loc"), Some(21450.0));
        assert_eq!(by_key(&m, "has_ci"), Some(1.0));
        assert_eq!(by_key(&m, "todo_count"), Some(7.0));
        assert!(by_key(&m, "complexity_avg").is_none(), "null is omitted");
        assert!(by_key(&m, "duplication_pct").is_none(), "absent field is omitted");
    }

    #[test]
    fn parses_security_fields() {
        let text = "{\"vuln_critical\":1,\"vuln_high\":3,\"deps_outdated\":12,\"secrets_detected\":0}";
        let m = parse_fields(text, SECURITY_FIELDS);
        assert_eq!(by_key(&m, "vuln_critical"), Some(1.0));
        assert_eq!(by_key(&m, "vuln_high"), Some(3.0));
        assert_eq!(by_key(&m, "deps_outdated"), Some(12.0));
        assert_eq!(by_key(&m, "secrets_detected"), Some(0.0));
        assert!(by_key(&m, "vuln_low").is_none());
    }

    #[test]
    fn parse_fields_handles_no_json() {
        assert!(parse_fields("no json here", HEALTH_FIELDS).is_empty());
    }

    #[test]
    fn derives_architecture_and_model_coverage_metrics() {
        let detail = serde_json::json!({
            "dependencies": [
                { "target_name": "billing", "target_app_id": "11111111-1111-1111-1111-111111111111" },
                { "target_name": "stripe", "target_app_id": null }
            ],
            "components": [
                { "name": "api", "observability_signals": [ { "name": "p99" }, { "name": "errors" } ] },
                { "name": "worker", "observability_signals": [] }
            ],
            "use_cases": [
                { "name": "checkout", "diagrams": [ { "kind": "sequence" } ] }
            ]
        });
        let m = derived_metrics(&detail);
        assert_eq!(by_key(&m, "fan_out"), Some(2.0));
        assert_eq!(by_key(&m, "external_dependency_count"), Some(1.0));
        assert_eq!(by_key(&m, "component_count"), Some(2.0));
        assert_eq!(by_key(&m, "use_case_count"), Some(1.0));
        assert_eq!(by_key(&m, "observability_signal_count"), Some(2.0));
        assert_eq!(by_key(&m, "has_use_cases"), Some(1.0));
        assert_eq!(by_key(&m, "has_diagrams"), Some(1.0));
    }

    #[test]
    fn derived_metrics_handle_empty_model() {
        let m = derived_metrics(&serde_json::json!({}));
        assert_eq!(by_key(&m, "fan_out"), Some(0.0));
        assert_eq!(by_key(&m, "has_use_cases"), Some(0.0));
        assert_eq!(by_key(&m, "has_diagrams"), Some(0.0));
    }
}
