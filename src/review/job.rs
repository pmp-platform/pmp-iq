//! The `review-repositories` job: clone selected repositories from every
//! enabled account, then (when an AI profile is configured) analyse each and
//! persist the results into the platform model.

use crate::accounts::{AccountService, ProviderType, RemoteRepo, RepositoryAccount};
use crate::ai::{AiProfileService, AiProvider, AiProviderDeps, AiProviderFactory};
use crate::analysis_config::AnalysisConfigService;
use crate::cost::{BudgetGuard, BudgetScope, LlmBudgetRepository, LlmUsageRepository, ScopeRef};
use crate::db::RepoResult;
use crate::error::AppError;
use crate::git::{CloneRequest, GitClient};
use crate::hints::{EntityHint, EntityHintRepository};
use crate::jobs::model::{Job, JobInput, TriggerType, merge_object};
use crate::jobs::repository::JobRepository;
use crate::jobs::{JobContext, JobError, JobOutcome, JobType, UsageAttribution, enforce_budget};
use crate::locks::{DistributedLock, Lease, lock_keys};
use crate::platform::catalog::resolve_dependencies;
use crate::platform::{
    AnalysisInput, AnalysisResult, Catalog, MemberInfo, PlatformQuery, PlatformWriter,
    RepositoryAnalyzer,
};
use crate::repositories::{RepoRecord, RepoRecordInput, RepoRecordRepository};
use crate::workspace::{JobLocator, Workspace};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub const JOB_TYPE: &str = "sync-repositories";

/// Lease length for the per-job sync lock; refreshed between accounts.
const LOCK_TTL: Duration = Duration::from_secs(120);

/// Dependencies for the review job (bundled to bound parameter count).
#[derive(Clone)]
pub struct ReviewDeps {
    pub accounts: AccountService,
    pub repositories: Arc<dyn RepoRecordRepository>,
    pub git: Arc<dyn GitClient>,
    pub workspace: Workspace,
    pub analyzer: Arc<dyn RepositoryAnalyzer>,
    pub writer: Arc<dyn PlatformWriter>,
    pub platform: Arc<dyn PlatformQuery>,
    pub ai: AiProfileService,
    pub ai_deps: AiProviderDeps,
    pub analysis_config: AnalysisConfigService,
    pub lock: Arc<dyn DistributedLock>,
    pub hints: Arc<dyn EntityHintRepository>,
    /// LLM usage recording + budget enforcement (M39).
    pub usage: Arc<dyn LlmUsageRepository>,
    pub budgets: Arc<dyn LlmBudgetRepository>,
    pub budget: Arc<BudgetGuard>,
}

/// A resolved AI provider plus the profile/model backing it (for usage cost).
struct ResolvedProvider {
    provider: Box<dyn AiProvider>,
    profile_id: Uuid,
    model: String,
}

/// Running tally for the job summary (also persisted in the resume checkpoint).
#[derive(Default, Clone, Serialize, Deserialize)]
struct Tally {
    repositories: u32,
    cloned: u32,
    failed: u32,
    analyzed: u32,
    analysis_failed: u32,
}

/// Resume checkpoint: which accounts are fully processed, plus the tally.
#[derive(Default, Serialize, Deserialize)]
struct Checkpoint {
    done_accounts: HashSet<String>,
    tally: Tally,
}

impl Checkpoint {
    fn load(state: &Value) -> Self {
        serde_json::from_value(state.clone()).unwrap_or_default()
    }

    fn as_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }
}

/// One repository to clone (bundled to bound parameter count).
struct CloneTask<'a> {
    account: &'a RepositoryAccount,
    remote: &'a RemoteRepo,
    token: Option<String>,
}

/// AI context threaded through the clone/analyze path: the provider plus the
/// run-start catalog snapshot used to canonicalize dependency targets.
struct AnalysisCtx<'a> {
    provider: Option<&'a dyn AiProvider>,
    catalog: Option<&'a Catalog>,
    /// When set, sync only the repository with this full name (per-app sync).
    repo_filter: Option<String>,
}

/// Inputs for analysing one cloned repository (bundled to bound parameters).
struct AnalyzeInput<'a> {
    record: &'a RepoRecord,
    checkout_path: &'a str,
    account: &'a RepositoryAccount,
    provider: &'a dyn AiProvider,
    catalog: Option<&'a Catalog>,
    /// The freshly-cloned HEAD commit (M41), recorded as last-analyzed on success.
    head_sha: String,
}

/// Clones and analyses repositories from configured accounts.
pub struct ReviewRepositoriesJob {
    deps: ReviewDeps,
}

impl ReviewRepositoriesJob {
    pub fn new(deps: ReviewDeps) -> Self {
        Self { deps }
    }

    /// Build the AI provider for the job: the profile pinned in the job config,
    /// else the default profile. `None` only when no profile exists anywhere.
    async fn build_provider(
        &self,
        ctx: &JobContext,
    ) -> Result<Option<ResolvedProvider>, JobError> {
        let Some(id) = resolve_profile_id(&self.deps.ai, &ctx.config).await? else {
            return Ok(None);
        };
        let profile = self
            .deps
            .ai
            .get(id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let provider = AiProviderFactory::build(&profile, &self.deps.ai_deps)
            .map_err(|e| JobError::Failed(e.to_string()))?;
        Ok(Some(ResolvedProvider { provider, profile_id: id, model: profile.model() }))
    }

    /// Process one account. A rate-limit error is returned verbatim
    /// (`AppError::RateLimited`) so the caller can self-pause.
    async fn process_account(
        &self,
        ctx: &JobContext,
        account: &RepositoryAccount,
        analysis: &AnalysisCtx<'_>,
        tally: &mut Tally,
    ) -> Result<(), AppError> {
        let selected = self.deps.accounts.select_for(account).await?;
        let token = self.deps.accounts.clone_token(account)?;

        for remote in &selected {
            if let Some(filter) = &analysis.repo_filter {
                if &remote.full_name != filter {
                    continue;
                }
            }
            tally.repositories += 1;
            let task = CloneTask {
                account,
                remote,
                token: token.clone(),
            };
            self.clone_one(ctx, task, analysis, tally).await;
        }
        Ok(())
    }

    /// Upsert, clone, and (optionally) analyse a single repository. All errors
    /// are isolated to this repository.
    async fn clone_one(
        &self,
        ctx: &JobContext,
        task: CloneTask<'_>,
        analysis: &AnalysisCtx<'_>,
        tally: &mut Tally,
    ) {
        let record = match self.upsert(task.account.id, task.remote).await {
            Ok(record) => record,
            Err(message) => {
                tally.failed += 1;
                ctx.log(&format!("FAILED {}: {message}", task.remote.full_name)).await;
                return;
            }
        };
        match self.do_clone(ctx, &record, task.remote, task.token).await {
            Ok((path, head_sha)) => {
                tally.cloned += 1;
                if let Some(provider) = analysis.provider {
                    let input = AnalyzeInput {
                        record: &record,
                        checkout_path: &path,
                        account: task.account,
                        provider,
                        catalog: analysis.catalog,
                        head_sha,
                    };
                    self.analyze(ctx, input, tally).await;
                }
            }
            Err(message) => {
                tally.failed += 1;
                ctx.log(&format!("FAILED {}: {message}", task.remote.full_name)).await;
            }
        }
    }

    async fn do_clone(
        &self,
        ctx: &JobContext,
        record: &RepoRecord,
        remote: &RemoteRepo,
        token: Option<String>,
    ) -> Result<(String, String), String> {
        let dest = self
            .deps
            .workspace
            .repo_dir(&JobLocator::new(&ctx.job_name, ctx.job_id), &remote.full_name)
            .map_err(|e| e.to_string())?;
        let info = self
            .deps
            .git
            .clone_or_update(CloneRequest {
                clone_url: remote.clone_url.clone(),
                dest,
                branch: remote.default_branch.clone(),
                token,
            })
            .await
            .map_err(|e| e.to_string())?;
        self.deps
            .repositories
            .mark_cloned(record.id, &info.path, &info.commit_sha)
            .await
            .map_err(|e| e.to_string())?;
        ctx.log(&format!("cloned {}", remote.full_name)).await;
        Ok((info.path, info.commit_sha))
    }

    /// Decide full vs incremental analysis for this repo (M41) and, for
    /// incremental, the changed file set. Full unless the job is in incremental
    /// mode, a prior analyzed commit exists, the HEAD advanced, the base is
    /// reachable, and no structural file changed.
    async fn plan_mode(&self, ctx: &JobContext, input: &AnalyzeInput<'_>) -> (crate::incremental::Mode, Vec<String>) {
        let incremental = ctx.params.get("incremental").and_then(Value::as_bool).unwrap_or(false);
        let Some(last) = input.record.last_analyzed_sha.as_deref() else {
            return (crate::incremental::Mode::Full, vec![]);
        };
        if !incremental || last == input.head_sha {
            return (crate::incremental::Mode::Full, vec![]);
        }
        match self
            .deps
            .git
            .changed_files(input.checkout_path.to_string(), last.to_string(), input.head_sha.clone())
            .await
        {
            Ok(cf) => {
                let mode = crate::incremental::decide(Some(last), cf.base_missing, &cf.paths);
                ctx.log(&format!(
                    "{} analysis for {} ({} changed file(s))",
                    if matches!(mode, crate::incremental::Mode::Incremental) { "incremental" } else { "full" },
                    input.record.full_name,
                    cf.paths.len()
                ))
                .await;
                (mode, cf.paths)
            }
            Err(e) => {
                ctx.log(&format!("changed-file diff failed for {} — full analysis: {e}", input.record.full_name)).await;
                (crate::incremental::Mode::Full, vec![])
            }
        }
    }

    /// Resolve an optional per-execution scope: when `params.repository_id` is
    /// set, sync only that repository — returns its `(account_id, full_name)`.
    async fn sync_target(&self, params: &Value) -> Option<(Uuid, String)> {
        let id = params.get("repository_id").and_then(|v| v.as_str())?;
        let id = Uuid::parse_str(id).ok()?;
        let record = self.deps.repositories.get(id).await.ok()?;
        Some((record.account_id, record.full_name))
    }

    /// Load the user-configured hints for the application this repository maps
    /// to (empty when the application doesn't exist yet, i.e. first sync).
    async fn hints_for(&self, repository_id: Uuid) -> Vec<EntityHint> {
        match self.deps.platform.repository_application(repository_id).await {
            Ok(Some(app_id)) => self.deps.hints.list_for_application(app_id).await.unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    async fn analyze(&self, ctx: &JobContext, input: AnalyzeInput<'_>, tally: &mut Tally) {
        let record = input.record;
        let (mode, changed) = self.plan_mode(ctx, &input).await;
        let incremental = matches!(mode, crate::incremental::Mode::Incremental);
        let cfg = self.deps.analysis_config.load().await.unwrap_or_default();
        let analysis = AnalysisInput {
            checkout_path: input.checkout_path.to_string(),
            repo_full_name: record.full_name.clone(),
            provider: input.provider,
            config: cfg.clone(),
            hints: self.hints_for(record.id).await,
            changed_files: incremental.then_some(changed),
        };
        let mut result = match self.deps.analyzer.analyze(analysis).await {
            Ok(mut result) => {
                result.apply_config(&cfg);
                result
            }
            Err(e) => {
                tally.analysis_failed += 1;
                ctx.log(&format!("ANALYSIS FAILED {}: {e}", record.full_name)).await;
                return;
            }
        };
        // Dependency canonicalisation + member reconcile run on full analyses
        // only; incremental syncs touch only the affected app sub-entities.
        if !incremental {
            if let Some(catalog) = input.catalog {
                let n = resolve_dependencies(&mut result, catalog, Some(input.provider)).await;
                if n > 0 {
                    ctx.log(&format!("resolved {n} dependency target(s) for {}", record.full_name)).await;
                }
            }
        }
        match self.persist(ctx, &input, incremental, &result).await {
            Ok(app_id) => {
                let _ = self.deps.repositories.mark_reviewed(record.id).await;
                let _ = self.deps.repositories.mark_analyzed(record.id, &input.head_sha).await;
                tally.analyzed += 1;
                ctx.log(&format!("analysed {}", record.full_name)).await;
                if !incremental {
                    self.reconcile_members(ctx, input.account, record, app_id).await;
                }
            }
            Err(e) => {
                tally.analysis_failed += 1;
                ctx.log(&format!("ANALYSIS WRITE FAILED {}: {e}", record.full_name)).await;
            }
        }
    }

    /// Write the result: a partial merge for an incremental sync of an existing
    /// application, else a full write. Returns the application id.
    async fn persist(
        &self,
        _ctx: &JobContext,
        input: &AnalyzeInput<'_>,
        incremental: bool,
        result: &AnalysisResult,
    ) -> RepoResult<Uuid> {
        if incremental {
            if let Some(app_id) = self.deps.platform.repository_application(input.record.id).await? {
                self.deps.writer.write_partial(app_id, result).await?;
                return Ok(app_id);
            }
        }
        self.deps.writer.write(input.record.id, result).await
    }

    /// Fetch the repository's current members from the provider and reconcile
    /// them into the platform model (members / ex-members). Scoped to providers
    /// that expose members (GitHub); failures are isolated and logged.
    async fn reconcile_members(
        &self,
        ctx: &JobContext,
        account: &RepositoryAccount,
        record: &RepoRecord,
        app_id: Uuid,
    ) {
        if account.provider_type != ProviderType::Github {
            return;
        }
        let members = match self.deps.accounts.members_for(account, &record.full_name).await {
            Ok(members) => members,
            Err(e) => {
                ctx.log(&format!("MEMBERS FAILED {}: {e}", record.full_name)).await;
                return;
            }
        };
        let mapped: Vec<MemberInfo> = members
            .into_iter()
            .map(|m| MemberInfo {
                username: m.username,
                email: m.email,
                role: m.role,
                permissions: m.permissions,
                metadata: json!({}),
            })
            .collect();
        match self.deps.writer.reconcile_members(app_id, &mapped).await {
            Ok(()) => ctx.log(&format!("{} member(s) for {}", mapped.len(), record.full_name)).await,
            Err(e) => ctx.log(&format!("MEMBERS WRITE FAILED {}: {e}", record.full_name)).await,
        }
    }

    async fn upsert(
        &self,
        account_id: Uuid,
        remote: &RemoteRepo,
    ) -> Result<RepoRecord, String> {
        self.deps
            .repositories
            .upsert(RepoRecordInput {
                account_id,
                name: remote.name.clone(),
                full_name: remote.full_name.clone(),
                clone_url: remote.clone_url.clone(),
                default_branch: remote.default_branch.clone(),
            })
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
impl JobType for ReviewRepositoriesJob {
    fn id(&self) -> &str {
        JOB_TYPE
    }

    fn description(&self) -> &str {
        "Synchronize selected repositories from all enabled accounts: clone each one and, \
         when an AI profile is set, analyze it into the platform model (removing stale data \
         it no longer produces). Without an AI profile, repositories are only cloned."
    }

    async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError> {
        // Serialise repository syncs across instances with a per-job lock.
        let key = lock_keys::job(ctx.job_id);
        let mut lease = match self.deps.lock.acquire(&key, LOCK_TTL).await {
            Ok(Some(lease)) => lease,
            Ok(None) => {
                ctx.log("another instance holds the sync lock — rescheduling").await;
                return Err(JobError::CannotRun { retry_at: None });
            }
            Err(e) => return Err(JobError::Failed(format!("sync lock error: {e}"))),
        };
        let outcome = self.sweep(&ctx, &mut lease).await;
        let _ = self.deps.lock.release(&lease).await;
        outcome
    }
}

impl ReviewRepositoriesJob {
    /// The locked sweep: clone + analyse every enabled account's repositories,
    /// refreshing the lock lease between accounts.
    async fn sweep(&self, ctx: &JobContext, lease: &mut Lease) -> Result<JobOutcome, JobError> {
        // Wrap the provider so all analysis prompts/responses are written to the
        // execution output and token usage is recorded in its metadata + priced
        // into the cost table (M39).
        let recorder = match self.build_provider(ctx).await? {
            Some(resolved) => {
                let scopes = [
                    ScopeRef::new(BudgetScope::Profile, Some(resolved.profile_id)),
                    ScopeRef::new(BudgetScope::Job, Some(ctx.job_id)),
                ];
                enforce_budget(&self.deps.budget, self.deps.budgets.as_ref(), &scopes, ctx).await?;
                let attribution = UsageAttribution {
                    application_id: None,
                    ai_profile_id: Some(resolved.profile_id),
                    model: resolved.model,
                };
                Some(ctx.recording_provider_priced(resolved.provider, self.deps.usage.clone(), attribution))
            }
            None => None,
        };
        let provider: Option<&dyn AiProvider> = recorder.as_ref().map(|r| r as &dyn AiProvider);
        // Snapshot the known-entity catalog once (best-effort) so analysis can
        // canonicalize dependency targets without re-querying per repo.
        let catalog = if provider.is_some() {
            match self.deps.platform.catalog().await {
                Ok(catalog) => Some(catalog),
                Err(e) => {
                    ctx.log(&format!("catalog snapshot failed: {e}")).await;
                    None
                }
            }
        } else {
            None
        };
        // Optional per-execution scope: sync only one repository (per-app sync).
        let target = self.sync_target(&ctx.params).await;
        let analysis_ctx = AnalysisCtx {
            provider,
            catalog: catalog.as_ref(),
            repo_filter: target.as_ref().map(|(_, full_name)| full_name.clone()),
        };
        let accounts = self
            .deps
            .accounts
            .list_enabled()
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;

        // Resume from any prior checkpoint (skips already-processed accounts).
        let mut checkpoint = Checkpoint::load(&ctx.state);
        let analysis = if provider.is_some() {
            "with analysis".to_string()
        } else {
            "without analysis (no AI profile configured — add one under AI Profiles)".to_string()
        };
        ctx.log(&format!(
            "reviewing {} account(s) {analysis} ({} already done)",
            accounts.len(),
            checkpoint.done_accounts.len()
        ))
        .await;

        for account in &accounts {
            // When scoped to a single repository, skip every other account.
            if let Some((account_id, _)) = &target {
                if account.id != *account_id {
                    continue;
                }
            }
            let key = account.id.to_string();
            if checkpoint.done_accounts.contains(&key) {
                continue;
            }
            // Cooperative manual pause: stop cleanly between accounts.
            if ctx.pause_requested().await {
                ctx.log("pause requested — pausing").await;
                return Ok(JobOutcome::Paused {
                    state: checkpoint.as_value(),
                    resume_at: None,
                });
            }
            match self
                .process_account(ctx, account, &analysis_ctx, &mut checkpoint.tally)
                .await
            {
                Ok(()) => {
                    checkpoint.done_accounts.insert(key);
                    ctx.save_state(&checkpoint.as_value()).await;
                    if let Ok(renewed) = self.deps.lock.refresh(lease, LOCK_TTL).await {
                        *lease = renewed;
                    }
                }
                // Rate limited: self-pause until the limit resets.
                Err(AppError::RateLimited { retry_at }) => {
                    ctx.log("rate limited — self-pausing until reset").await;
                    return Ok(JobOutcome::Paused {
                        state: checkpoint.as_value(),
                        resume_at: retry_at,
                    });
                }
                Err(e) => return Err(JobError::Failed(format!("account '{}': {e}", account.name))),
            }
        }

        // Re-analysis can orphan shared entities (a library a repo no longer
        // uses); prune them once the full sweep completes. Best-effort.
        if provider.is_some() {
            match self.deps.writer.prune_orphans().await {
                Ok(()) => ctx.log("pruned orphaned shared entities").await,
                Err(e) => ctx.log(&format!("orphan prune failed: {e}")).await,
            }
        }

        let t = &checkpoint.tally;
        ctx.log(&format!(
            "done: {} repositories, {} cloned, {} failed, {} analysed",
            t.repositories, t.cloned, t.failed, t.analyzed
        ))
        .await;
        Ok(JobOutcome::completed(json!({
            "accounts": accounts.len(),
            "repositories": t.repositories,
            "cloned": t.cloned,
            "failed": t.failed,
            "analyzed": t.analyzed,
            "analysis_failed": t.analysis_failed,
        })))
    }
}

/// The profile id to analyse with: the one pinned in the job `config`, else the
/// default profile (so a sync seeded without a profile still analyses when one
/// exists). `None` only when no profile is configured anywhere.
async fn resolve_profile_id(
    ai: &AiProfileService,
    config: &Value,
) -> Result<Option<Uuid>, JobError> {
    if let Some(s) = config.get("ai_profile_id").and_then(|v| v.as_str()) {
        let id = Uuid::parse_str(s)
            .map_err(|e| JobError::Failed(format!("invalid ai_profile_id: {e}")))?;
        return Ok(Some(id));
    }
    ai.default_profile_id()
        .await
        .map_err(|e| JobError::Failed(e.to_string()))
}

/// The sync-job config, carrying `ai_profile_id` when a profile is available.
fn sync_config(ai_profile_id: Option<Uuid>) -> Value {
    match ai_profile_id {
        Some(id) => json!({ "ai_profile_id": id }),
        None => json!({}),
    }
}

/// Find the singleton `sync-repositories` job, creating it (pre-seeded at boot)
/// if absent. When it already exists but carries no `ai_profile_id` and one is
/// now available, backfill it so later syncs run analysis. Returns its id so a
/// per-application sync can be enqueued with a `repository_id` scope.
pub async fn ensure_sync_job(
    jobs: &dyn JobRepository,
    ai_profile_id: Option<Uuid>,
) -> Result<Uuid, AppError> {
    let existing = jobs.list().await?;
    if let Some(job) = existing.iter().find(|j| j.job_type == JOB_TYPE) {
        backfill_profile(jobs, job, ai_profile_id).await?;
        return Ok(job.id);
    }
    let created = jobs
        .create(JobInput {
            job_type: JOB_TYPE.to_string(),
            name: "Sync repositories".to_string(),
            trigger_type: TriggerType::Manual,
            cron_expr: None,
            config: sync_config(ai_profile_id),
            enabled: true,
            next_run_at: None,
        })
        .await?;
    Ok(created.id)
}

/// Add `ai_profile_id` to an existing sync job that lacks one, preserving its
/// other settings. No-op when the job already has a profile or none is given.
async fn backfill_profile(
    jobs: &dyn JobRepository,
    job: &Job,
    ai_profile_id: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(id) = ai_profile_id else {
        return Ok(());
    };

    if job.config.get("ai_profile_id").and_then(Value::as_str).is_some() {
        return Ok(());
    }
    let mut config = job.config.clone();
    merge_object(&mut config, &json!({ "ai_profile_id": id }));
    jobs.update(
        job.id,
        JobInput {
            job_type: job.job_type.clone(),
            name: job.name.clone(),
            trigger_type: job.trigger_type,
            cron_expr: job.cron_expr.clone(),
            config,
            enabled: job.enabled,
            next_run_at: job.next_run_at,
        },
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod ensure_sync_job_tests {
    use super::*;
    use crate::jobs::repository::MockJobRepository;

    fn job_from_input(i: JobInput) -> Job {
        Job {
            id: Uuid::new_v4(),
            job_type: i.job_type,
            name: i.name,
            trigger_type: i.trigger_type,
            cron_expr: i.cron_expr,
            config: i.config,
            enabled: i.enabled,
            next_run_at: i.next_run_at,
        }
    }

    fn seed_job(id: Uuid, config: Value) -> Job {
        Job {
            id,
            job_type: JOB_TYPE.into(),
            name: "Sync repositories".into(),
            trigger_type: TriggerType::Manual,
            cron_expr: None,
            config,
            enabled: true,
            next_run_at: None,
        }
    }

    #[tokio::test]
    async fn creates_seed_job_with_profile() {
        let profile = Uuid::new_v4();
        let mut jobs = MockJobRepository::new();
        jobs.expect_list().times(1).returning(|| Ok(vec![]));
        jobs.expect_create()
            .times(1)
            .withf(move |i| {
                i.job_type == JOB_TYPE
                    && i.config.get("ai_profile_id").and_then(Value::as_str)
                        == Some(profile.to_string().as_str())
            })
            .returning(|i| Ok(job_from_input(i)));
        assert!(ensure_sync_job(&jobs, Some(profile)).await.is_ok());
    }

    #[tokio::test]
    async fn backfills_missing_profile_preserving_config() {
        let profile = Uuid::new_v4();
        let job_id = Uuid::new_v4();
        let existing = seed_job(job_id, json!({ "max_concurrency": 2 }));
        let mut jobs = MockJobRepository::new();
        jobs.expect_list().times(1).returning(move || Ok(vec![existing.clone()]));
        jobs.expect_update()
            .times(1)
            .withf(move |id, i| {
                *id == job_id
                    && i.config.get("ai_profile_id").and_then(Value::as_str)
                        == Some(profile.to_string().as_str())
                    && i.config.get("max_concurrency").and_then(Value::as_u64) == Some(2)
            })
            .returning(|_, i| Ok(job_from_input(i)));
        assert_eq!(ensure_sync_job(&jobs, Some(profile)).await.unwrap(), job_id);
    }

    #[tokio::test]
    async fn keeps_existing_profile_and_does_not_update() {
        let job_id = Uuid::new_v4();
        let existing = seed_job(job_id, json!({ "ai_profile_id": Uuid::new_v4() }));
        let mut jobs = MockJobRepository::new();
        jobs.expect_list().times(1).returning(move || Ok(vec![existing.clone()]));
        // No expect_update / expect_create: the existing profile is untouched.
        assert_eq!(ensure_sync_job(&jobs, Some(Uuid::new_v4())).await.unwrap(), job_id);
    }

    #[tokio::test]
    async fn no_profile_leaves_existing_job_untouched() {
        let job_id = Uuid::new_v4();
        let existing = seed_job(job_id, json!({}));
        let mut jobs = MockJobRepository::new();
        jobs.expect_list().times(1).returning(move || Ok(vec![existing.clone()]));
        // None profile → no backfill, no update.
        assert_eq!(ensure_sync_job(&jobs, None).await.unwrap(), job_id);
    }
}

#[cfg(test)]
mod resolve_profile_id_tests {
    use super::*;
    use crate::ai::AiProviderDeps;
    use crate::ai::model::{AiProfile, AiProviderType};
    use crate::ai::repository::MockAiProfileRepository;
    use crate::crypto::MockEncryptor;
    use crate::httpclient::MockHttpClient;
    use crate::process::MockCommandRunner;

    fn ai_service(repo: MockAiProfileRepository) -> AiProfileService {
        let deps = AiProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            runner: Arc::new(MockCommandRunner::new()),
            encryptor: Arc::new(MockEncryptor::new()),
        };
        AiProfileService::new(Arc::new(repo), deps)
    }

    fn profile_with(id: Uuid, enabled: bool) -> AiProfile {
        AiProfile {
            id,
            name: "p".into(),
            provider_type: AiProviderType::ClaudeCli,
            config: json!({ "binary_path": "claude" }),
            secrets_enc: None,
            enabled,
        }
    }

    #[tokio::test]
    async fn uses_pinned_profile_without_consulting_repo() {
        let id = Uuid::new_v4();
        // No `expect_list`: a pinned id must not trigger a default lookup.
        let ai = ai_service(MockAiProfileRepository::new());
        let config = json!({ "ai_profile_id": id });
        assert_eq!(resolve_profile_id(&ai, &config).await.unwrap(), Some(id));
    }

    #[tokio::test]
    async fn falls_back_to_default_profile_when_unset() {
        let want = Uuid::new_v4();
        let mut repo = MockAiProfileRepository::new();
        repo.expect_list().times(1).returning(move || Ok(vec![profile_with(want, true)]));
        let ai = ai_service(repo);
        assert_eq!(resolve_profile_id(&ai, &json!({})).await.unwrap(), Some(want));
    }

    #[tokio::test]
    async fn returns_none_when_no_profiles_exist() {
        let mut repo = MockAiProfileRepository::new();
        repo.expect_list().times(1).returning(|| Ok(vec![]));
        let ai = ai_service(repo);
        assert_eq!(resolve_profile_id(&ai, &json!({})).await.unwrap(), None);
    }

    #[tokio::test]
    async fn invalid_pinned_id_errors() {
        let ai = ai_service(MockAiProfileRepository::new());
        let config = json!({ "ai_profile_id": "not-a-uuid" });
        assert!(matches!(
            resolve_profile_id(&ai, &config).await,
            Err(JobError::Failed(_))
        ));
    }
}
