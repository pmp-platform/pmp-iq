//! The `llm-repository-request` job type.

use crate::accounts::AccountService;
use crate::ai::{AiProfileService, AiProvider, AiProviderDeps, AiProviderFactory, AiRequest};
use crate::cost::LlmUsageRepository;
use crate::error::AppError;
use crate::git::{CloneRequest, GitClient};
use crate::jobs::repository::JobRepository;
use crate::jobs::{JobContext, JobError, JobOutcome, JobType, RecordingAiProvider, UsageAttribution};
use crate::jobs::model::{JobInput, TriggerType};
use crate::locks::{DistributedLock, lock_keys};
use crate::repositories::{RepoRecord, RepoRecordRepository};
use crate::workspace::{JobLocator, Workspace};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub const JOB_TYPE: &str = "llm-repository-request";

/// Lease length for the per-repository lock; the request runs within it.
const LOCK_TTL: Duration = Duration::from_secs(600);

/// Dependencies for the job (bundled to bound parameter count).
#[derive(Clone)]
pub struct LlmRequestDeps {
    pub accounts: AccountService,
    pub repositories: Arc<dyn RepoRecordRepository>,
    pub git: Arc<dyn GitClient>,
    pub workspace: Workspace,
    pub ai: AiProfileService,
    pub ai_deps: AiProviderDeps,
    pub lock: Arc<dyn DistributedLock>,
    /// LLM usage recording for cost rollups (M39).
    pub usage: Arc<dyn LlmUsageRepository>,
}

/// Per-execution input carried on `job_executions.params`.
#[derive(Deserialize)]
struct RequestInput {
    /// The repository record id to query.
    repository: Uuid,
    #[serde(default)]
    branch: Option<String>,
    /// The user's question / instruction for the LLM.
    input: String,
    /// The AI agent profile to run with.
    ai_profile_id: Uuid,
}

/// Runs an LLM session over one repository checkout.
pub struct LlmRepositoryRequestJob {
    deps: LlmRequestDeps,
}

impl LlmRepositoryRequestJob {
    pub fn new(deps: LlmRequestDeps) -> Self {
        Self { deps }
    }

    fn parse_input(ctx: &JobContext) -> Result<RequestInput, JobError> {
        serde_json::from_value(ctx.params.clone())
            .map_err(|e| JobError::Failed(format!("invalid llm-request params: {e}")))
    }

    fn branch_of(record: &RepoRecord, input: &RequestInput) -> String {
        input
            .branch
            .clone()
            .or_else(|| record.default_branch.clone())
            .unwrap_or_else(|| "main".to_string())
    }

    /// Clone-if-missing then fetch+rebase the branch; returns the checkout path.
    async fn checkout(
        &self,
        ctx: &JobContext,
        record: &RepoRecord,
        branch: &str,
    ) -> Result<String, JobError> {
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
        let info = self
            .deps
            .git
            .sync_branch(CloneRequest {
                clone_url: record.clone_url.clone(),
                dest,
                branch: Some(branch.to_string()),
                token,
            })
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let _ = self.deps.repositories.mark_cloned(record.id, &info.path, &info.commit_sha).await;
        Ok(info.path)
    }

    /// Build the configured provider wrapped so its I/O is recorded.
    async fn provider(
        &self,
        ctx: &JobContext,
        profile_id: Uuid,
    ) -> Result<RecordingAiProvider, JobError> {
        let profile = self
            .deps
            .ai
            .get(profile_id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let inner = AiProviderFactory::build(&profile, &self.deps.ai_deps)
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let attribution = UsageAttribution {
            application_id: None,
            ai_profile_id: Some(profile_id),
            model: profile.model(),
        };
        Ok(ctx.recording_provider_priced(inner, self.deps.usage.clone(), attribution))
    }

    /// Sync the checkout, run the LLM, and assemble the outcome.
    async fn answer(
        &self,
        ctx: &JobContext,
        record: &RepoRecord,
        input: &RequestInput,
    ) -> Result<JobOutcome, JobError> {
        let branch = Self::branch_of(record, input);
        ctx.log(&format!("syncing {} ({branch})", record.full_name)).await;
        let path = self.checkout(ctx, record, &branch).await?;

        let provider = self.provider(ctx, input.ai_profile_id).await?;
        let system = format!(
            "You are answering a question about the repository '{}'. Its files are checked out in \
             your working directory — inspect them as needed. Answer accurately and concisely.",
            record.full_name
        );
        let request = AiRequest::new(input.input.clone())
            .with_system(system)
            .with_working_dir(path);
        // The runner heartbeats the execution in the background for its whole
        // duration, so this single long LLM call won't be cancelled mid-run.
        let response = provider
            .complete(request)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;

        ctx.merge_metadata(&json!({
            "answer": response.text,
            "repository": record.full_name,
            "branch": branch,
        }))
        .await;
        Ok(JobOutcome::completed(json!({
            "repository": record.full_name,
            "branch": branch,
            "answer": response.text,
            "input_tokens": response.input_tokens,
            "output_tokens": response.output_tokens,
        })))
    }
}

#[async_trait]
impl JobType for LlmRepositoryRequestJob {
    fn id(&self) -> &str {
        JOB_TYPE
    }

    fn description(&self) -> &str {
        "Clone (or update) a repository and run an LLM session with a supplied input against the \
         checkout, returning the answer. Serialised per repository with a distributed lock."
    }

    async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError> {
        let input = Self::parse_input(&ctx)?;
        let record = self
            .deps
            .repositories
            .get(input.repository)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;

        // Serialise per repository: a concurrent request reschedules.
        let key = lock_keys::repository(&record.full_name);
        let lease = match self.deps.lock.acquire(&key, LOCK_TTL).await {
            Ok(Some(lease)) => lease,
            Ok(None) => {
                ctx.log("repository is busy — rescheduling").await;
                return Err(JobError::CannotRun { retry_at: None });
            }
            Err(e) => return Err(JobError::Failed(format!("repository lock error: {e}"))),
        };
        let outcome = self.answer(&ctx, &record, &input).await;
        let _ = self.deps.lock.release(&lease).await;
        outcome
    }
}

/// Find the singleton `llm-repository-request` job, creating it if absent.
/// Returns its id so callers can enqueue executions with per-request params.
pub async fn ensure_job(jobs: &dyn JobRepository) -> Result<Uuid, AppError> {
    let existing = jobs.list().await?;
    if let Some(job) = existing.iter().find(|j| j.job_type == JOB_TYPE) {
        return Ok(job.id);
    }
    let created = jobs
        .create(JobInput {
            job_type: JOB_TYPE.to_string(),
            name: "Ask the repository".to_string(),
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
    use crate::accounts::ProviderDeps;
    use crate::accounts::repository::MockRepositoryAccountRepository;
    use crate::ai::repository::MockAiProfileRepository;
    use crate::crypto::MockEncryptor;
    use crate::fs::MockFileSystem;
    use crate::git::MockGitClient;
    use crate::httpclient::MockHttpClient;
    use crate::jobs::log_sink::MockLogSink;
    use crate::jobs::model::Job;
    use crate::jobs::repository::{MockJobExecutionRepository, MockJobRepository};
    use crate::locks::MockDistributedLock;
    use crate::process::MockCommandRunner;
    use crate::repositories::repository::MockRepoRecordRepository;
    use serde_json::Value;

    fn record() -> RepoRecord {
        RepoRecord {
            id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            name: "api".into(),
            full_name: "org/api".into(),
            clone_url: "https://x/org/api.git".into(),
            default_branch: Some("develop".into()),
            local_path: None,
            last_commit_sha: None,
            last_analyzed_sha: None,
        }
    }

    /// A permissive LLM-usage mock (recording is best-effort, never asserted).
    fn noop_usage() -> Arc<dyn crate::cost::LlmUsageRepository> {
        let mut m = crate::cost::repository::MockLlmUsageRepository::new();
        m.expect_record().returning(|_| Ok(()));
        Arc::new(m)
    }

    fn deps(repos: MockRepoRecordRepository, lock: MockDistributedLock) -> LlmRequestDeps {
        let provider_deps = ProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            fs: Arc::new(MockFileSystem::new()),
            encryptor: Arc::new(MockEncryptor::new()),
        };
        let ai_deps = AiProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            runner: Arc::new(MockCommandRunner::new()),
            encryptor: Arc::new(MockEncryptor::new()),
        };
        LlmRequestDeps {
            accounts: AccountService::new(Arc::new(MockRepositoryAccountRepository::new()), provider_deps),
            repositories: Arc::new(repos),
            git: Arc::new(MockGitClient::new()),
            workspace: Workspace::new(Arc::new(MockFileSystem::new()), "/w".into()),
            ai: AiProfileService::new(Arc::new(MockAiProfileRepository::new()), ai_deps.clone()),
            ai_deps,
            lock: Arc::new(lock),
            usage: noop_usage(),
        }
    }

    fn ctx_with(params: Value) -> JobContext {
        JobContext {
            execution_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            job_name: "ask".into(),
            config: json!({}),
            params,
            state: Value::Null,
            log: Arc::new(MockLogSink::new()),
            executions: Arc::new(MockJobExecutionRepository::new()),
            clock: Arc::new(crate::jobs::clock::SystemClock),
        }
    }

    #[test]
    fn branch_precedence_input_then_default_then_main() {
        let r = record();
        let req = |b: Option<&str>| RequestInput {
            repository: r.id,
            branch: b.map(|s| s.to_string()),
            input: "x".into(),
            ai_profile_id: Uuid::new_v4(),
        };
        assert_eq!(LlmRepositoryRequestJob::branch_of(&r, &req(Some("feat"))), "feat");
        assert_eq!(LlmRepositoryRequestJob::branch_of(&r, &req(None)), "develop");
        let mut bare = record();
        bare.default_branch = None;
        assert_eq!(LlmRepositoryRequestJob::branch_of(&bare, &req(None)), "main");
    }

    #[test]
    fn parse_input_rejects_missing_fields() {
        let ctx = ctx_with(json!({ "input": "hi" }));
        assert!(LlmRepositoryRequestJob::parse_input(&ctx).is_err());
    }

    #[tokio::test]
    async fn busy_repository_reschedules_without_failing() {
        let mut repos = MockRepoRecordRepository::new();
        repos.expect_get().returning(|_| Ok(record()));
        let mut lock = MockDistributedLock::new();
        lock.expect_acquire().returning(|_, _| Ok(None));
        let job = LlmRepositoryRequestJob::new(deps(repos, lock));

        let mut log = MockLogSink::new();
        log.expect_append().returning(|_, _| Ok(()));
        let ctx = JobContext {
            log: Arc::new(log),
            ..ctx_with(json!({
                "repository": Uuid::new_v4(),
                "input": "what does this do?",
                "ai_profile_id": Uuid::new_v4(),
            }))
        };
        assert!(matches!(job.run(ctx).await, Err(JobError::CannotRun { .. })));
    }

    #[tokio::test]
    async fn full_request_runs_llm_over_checkout_and_records_answer() {
        use crate::accounts::model::{AuthType, ProviderType, RepositoryAccount, SelectionMode};
        use crate::ai::AiProviderType;
        use crate::ai::model::AiProfile;
        use crate::git::CheckoutInfo;
        use crate::process::CommandOutput;

        // Repository + clone marking.
        let mut repos = MockRepoRecordRepository::new();
        repos.expect_get().returning(|_| Ok(record()));
        repos.expect_mark_cloned().returning(|_, _, _| Ok(()));

        // Account (GitHub, no stored credentials).
        let mut acct = MockRepositoryAccountRepository::new();
        acct.expect_get().returning(|id| {
            Ok(RepositoryAccount {
                id,
                name: "gh".into(),
                provider_type: ProviderType::Github,
                auth_type: AuthType::Token,
                base_url: None,
                organization: None,
                credentials_enc: None,
                selection_mode: SelectionMode::All,
                selection_value: None,
                enabled: true,
            })
        });
        let accounts = AccountService::new(
            Arc::new(acct),
            ProviderDeps {
                http: Arc::new(MockHttpClient::new()),
                fs: Arc::new(MockFileSystem::new()),
                encryptor: Arc::new(MockEncryptor::new()),
            },
        );

        // Git: a successful checkout.
        let mut git = MockGitClient::new();
        git.expect_sync_branch()
            .returning(|req| Ok(CheckoutInfo { commit_sha: "sha".into(), path: req.dest }));

        // AI: a Claude-CLI profile whose runner returns a JSON answer.
        let mut ai_repo = MockAiProfileRepository::new();
        ai_repo.expect_get().returning(|id| {
            Ok(AiProfile {
                id,
                name: "cli".into(),
                provider_type: AiProviderType::ClaudeCli,
                config: json!({ "binary_path": "claude" }),
                secrets_enc: None,
                enabled: true,
            })
        });
        let mut runner = MockCommandRunner::new();
        runner.expect_run().returning(|_| {
            Ok(CommandOutput {
                status: 0,
                stdout: r#"{"result":"It is a web API.","usage":{"input_tokens":3,"output_tokens":4}}"#
                    .into(),
                stderr: String::new(),
            })
        });
        let ai_deps = AiProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            runner: Arc::new(runner),
            encryptor: Arc::new(MockEncryptor::new()),
        };
        let ai = AiProfileService::new(Arc::new(ai_repo), ai_deps.clone());

        // Workspace fs + lock.
        let mut ws_fs = MockFileSystem::new();
        ws_fs.expect_create_dir_all().returning(|_| Ok(()));
        let mut lock = MockDistributedLock::new();
        lock.expect_acquire().returning(|key, _| {
            Ok(Some(crate::locks::Lease {
                key: key.to_string(),
                token: "tok".into(),
                expires_at: chrono::Utc::now(),
            }))
        });
        lock.expect_release().returning(|_| Ok(()));

        let deps = LlmRequestDeps {
            accounts,
            repositories: Arc::new(repos),
            git: Arc::new(git),
            workspace: Workspace::new(Arc::new(ws_fs), "/w".into()),
            ai,
            ai_deps,
            lock: Arc::new(lock),
            usage: noop_usage(),
        };
        let job = LlmRepositoryRequestJob::new(deps);

        let mut log = MockLogSink::new();
        log.expect_append().returning(|_, _| Ok(()));
        let mut execs = MockJobExecutionRepository::new();
        execs.expect_merge_metadata().returning(|_, _| Ok(()));
        let ctx = JobContext {
            log: Arc::new(log),
            executions: Arc::new(execs),
            ..ctx_with(json!({
                "repository": Uuid::new_v4(),
                "input": "what is this?",
                "ai_profile_id": Uuid::new_v4(),
            }))
        };
        assert!(job.run(ctx).await.is_ok(), "happy LLM request completes");
    }

    #[tokio::test]
    async fn ensure_job_creates_then_reuses() {
        let mut jobs = MockJobRepository::new();
        jobs.expect_list().times(1).returning(|| Ok(vec![]));
        jobs.expect_create().times(1).returning(|input| {
            Ok(Job {
                id: Uuid::new_v4(),
                job_type: input.job_type,
                name: input.name,
                trigger_type: input.trigger_type,
                cron_expr: input.cron_expr,
                config: input.config,
                enabled: input.enabled,
                next_run_at: input.next_run_at,
            })
        });
        let created = ensure_job(&jobs).await.unwrap();

        let mut jobs2 = MockJobRepository::new();
        jobs2.expect_list().returning(move || {
            Ok(vec![Job {
                id: created,
                job_type: JOB_TYPE.into(),
                name: "Ask the repository".into(),
                trigger_type: TriggerType::Manual,
                cron_expr: None,
                config: json!({}),
                enabled: true,
                next_run_at: None,
            }])
        });
        assert_eq!(ensure_job(&jobs2).await.unwrap(), created);
    }
}
