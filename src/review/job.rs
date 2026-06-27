//! The `review-repositories` job: clone selected repositories from every
//! enabled account, then (when an AI profile is configured) analyse each and
//! persist the results into the platform model.

use crate::accounts::{AccountService, ProviderType, RemoteRepo, RepositoryAccount};
use crate::ai::{AiProfileService, AiProvider, AiProviderDeps, AiProviderFactory};
use crate::analysis_config::AnalysisConfigService;
use crate::error::AppError;
use crate::git::{CloneRequest, GitClient};
use crate::jobs::{JobContext, JobError, JobOutcome, JobType};
use crate::platform::{AnalysisInput, MemberInfo, PlatformWriter, RepositoryAnalyzer};
use crate::repositories::{RepoRecord, RepoRecordInput, RepoRecordRepository};
use crate::workspace::Workspace;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

pub const JOB_TYPE: &str = "sync-repositories";

/// Dependencies for the review job (bundled to bound parameter count).
#[derive(Clone)]
pub struct ReviewDeps {
    pub accounts: AccountService,
    pub repositories: Arc<dyn RepoRecordRepository>,
    pub git: Arc<dyn GitClient>,
    pub workspace: Workspace,
    pub analyzer: Arc<dyn RepositoryAnalyzer>,
    pub writer: Arc<dyn PlatformWriter>,
    pub ai: AiProfileService,
    pub ai_deps: AiProviderDeps,
    pub analysis_config: AnalysisConfigService,
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

/// Inputs for analysing one cloned repository (bundled to bound parameters).
struct AnalyzeInput<'a> {
    record: &'a RepoRecord,
    checkout_path: &'a str,
    account: &'a RepositoryAccount,
    provider: &'a dyn AiProvider,
}

/// Clones and analyses repositories from configured accounts.
pub struct ReviewRepositoriesJob {
    deps: ReviewDeps,
}

impl ReviewRepositoriesJob {
    pub fn new(deps: ReviewDeps) -> Self {
        Self { deps }
    }

    /// Build the AI provider for the job, if an `ai_profile_id` is configured.
    async fn build_provider(
        &self,
        ctx: &JobContext,
    ) -> Result<Option<Box<dyn AiProvider>>, JobError> {
        let Some(profile_id) = ctx.config.get("ai_profile_id").and_then(|v| v.as_str()) else {
            return Ok(None);
        };
        let id = Uuid::parse_str(profile_id)
            .map_err(|e| JobError::Failed(format!("invalid ai_profile_id: {e}")))?;
        let profile = self
            .deps
            .ai
            .get(id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let provider = AiProviderFactory::build(&profile, &self.deps.ai_deps)
            .map_err(|e| JobError::Failed(e.to_string()))?;
        Ok(Some(provider))
    }

    /// Process one account. A rate-limit error is returned verbatim
    /// (`AppError::RateLimited`) so the caller can self-pause.
    async fn process_account(
        &self,
        ctx: &JobContext,
        account: &RepositoryAccount,
        provider: Option<&dyn AiProvider>,
        tally: &mut Tally,
    ) -> Result<(), AppError> {
        let selected = self.deps.accounts.select_for(account).await?;
        let token = self.deps.accounts.clone_token(account)?;

        for remote in &selected {
            tally.repositories += 1;
            let task = CloneTask {
                account,
                remote,
                token: token.clone(),
            };
            self.clone_one(ctx, task, provider, tally).await;
        }
        Ok(())
    }

    /// Upsert, clone, and (optionally) analyse a single repository. All errors
    /// are isolated to this repository.
    async fn clone_one(
        &self,
        ctx: &JobContext,
        task: CloneTask<'_>,
        provider: Option<&dyn AiProvider>,
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
            Ok(path) => {
                tally.cloned += 1;
                if let Some(provider) = provider {
                    let input = AnalyzeInput {
                        record: &record,
                        checkout_path: &path,
                        account: task.account,
                        provider,
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
    ) -> Result<String, String> {
        let dest = self
            .deps
            .workspace
            .repo_dir(&ctx.execution_id.to_string(), &remote.full_name)
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
        Ok(info.path)
    }

    async fn analyze(&self, ctx: &JobContext, input: AnalyzeInput<'_>, tally: &mut Tally) {
        let record = input.record;
        let cfg = self.deps.analysis_config.load().await.unwrap_or_default();
        let analysis = AnalysisInput {
            checkout_path: input.checkout_path.to_string(),
            repo_full_name: record.full_name.clone(),
            provider: input.provider,
            config: cfg.clone(),
        };
        let result = match self.deps.analyzer.analyze(analysis).await {
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
        match self.deps.writer.write(record.id, &result).await {
            Ok(app_id) => {
                let _ = self.deps.repositories.mark_reviewed(record.id).await;
                tally.analyzed += 1;
                ctx.log(&format!("analysed {}", record.full_name)).await;
                self.reconcile_members(ctx, input.account, record, app_id).await;
            }
            Err(e) => {
                tally.analysis_failed += 1;
                ctx.log(&format!("ANALYSIS WRITE FAILED {}: {e}", record.full_name)).await;
            }
        }
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
        let provider = self.build_provider(&ctx).await?;
        let accounts = self
            .deps
            .accounts
            .list_enabled()
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;

        // Resume from any prior checkpoint (skips already-processed accounts).
        let mut checkpoint = Checkpoint::load(&ctx.state);
        let analysis = if provider.is_some() { "with" } else { "without" };
        ctx.log(&format!(
            "reviewing {} account(s) {analysis} analysis ({} already done)",
            accounts.len(),
            checkpoint.done_accounts.len()
        ))
        .await;

        for account in &accounts {
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
                .process_account(&ctx, account, provider.as_deref(), &mut checkpoint.tally)
                .await
            {
                Ok(()) => {
                    checkpoint.done_accounts.insert(key);
                    ctx.save_state(&checkpoint.as_value()).await;
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
