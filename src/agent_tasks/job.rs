//! The `application-agent-task` job type: runs one agentic turn over an
//! application's repository — branch, edit, commit, push, open/update a PR.

use super::model::{AgentTask, AgentTaskMessage, AgentTaskTarget, NewMessage, role, status};
use super::repository::AgentTaskRepository;
use crate::accounts::{AccountService, PullRequestSpec, RepositoryAccount};
use crate::ai::{AiProfileService, AiProvider, AiProviderDeps, AiProviderFactory, AiRequest};
use crate::error::AppError;
use crate::git::{CloneRequest, CommitRequest, GitClient, PushRequest};
use crate::jobs::model::{JobInput, TriggerType};
use crate::jobs::repository::JobRepository;
use crate::jobs::{JobContext, JobError, JobOutcome, JobType, RecordingAiProvider};
use crate::locks::{DistributedLock, lock_keys};
use crate::repositories::{RepoRecord, RepoRecordRepository};
use crate::workspace::{JobLocator, Workspace};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub const JOB_TYPE: &str = "application-agent-task";

/// Lease length for the per-repository lock; the agent turn runs within it.
const LOCK_TTL: Duration = Duration::from_secs(1800);

/// Dependencies for the job (bundled to bound parameter count).
#[derive(Clone)]
pub struct AgentTaskDeps {
    pub tasks: Arc<dyn AgentTaskRepository>,
    pub accounts: AccountService,
    pub repositories: Arc<dyn RepoRecordRepository>,
    pub git: Arc<dyn GitClient>,
    pub workspace: Workspace,
    pub ai: AiProfileService,
    pub ai_deps: AiProviderDeps,
    pub lock: Arc<dyn DistributedLock>,
}

/// Per-execution input carried on `job_executions.params`.
#[derive(Deserialize)]
struct TurnInput {
    task_id: Uuid,
    /// The repository target this turn operates on (M23). Required.
    target_id: Uuid,
    /// The user's instruction (already persisted as a message by the route).
    #[allow(dead_code)]
    message: String,
    ai_profile_id: Uuid,
}

/// The branch/credentials used to check out and publish this turn.
struct CheckoutPlan {
    base: String,
    branch: String,
    token: Option<String>,
}

/// Resolved state shared across a turn's publish steps.
struct TurnState<'a> {
    task: &'a AgentTask,
    target: &'a AgentTaskTarget,
    record: &'a RepoRecord,
    account: RepositoryAccount,
    token: Option<String>,
    base: String,
    checkout: String,
}

/// Runs an agentic change session over one repository checkout.
pub struct AgentTaskJob {
    deps: AgentTaskDeps,
}

impl AgentTaskJob {
    pub fn new(deps: AgentTaskDeps) -> Self {
        Self { deps }
    }

    fn parse_input(ctx: &JobContext) -> Result<TurnInput, JobError> {
        serde_json::from_value(ctx.params.clone())
            .map_err(|e| JobError::Failed(format!("invalid agent-task params: {e}")))
    }

    fn base_branch(record: &RepoRecord) -> String {
        record.default_branch.clone().unwrap_or_else(|| "main".to_string())
    }

    async fn resolve_account(
        &self,
        record: &RepoRecord,
    ) -> Result<(RepositoryAccount, Option<String>), JobError> {
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
        Ok((account, token))
    }

    /// Sync the default branch, then create/checkout the task's own branch.
    async fn prepare_checkout(
        &self,
        ctx: &JobContext,
        record: &RepoRecord,
        plan: &CheckoutPlan,
    ) -> Result<String, JobError> {
        let dest = self
            .deps
            .workspace
            .repo_dir(&JobLocator::new(&ctx.job_name, ctx.job_id), &record.full_name)
            .map_err(|e| JobError::Failed(e.to_string()))?;
        self.deps
            .git
            .sync_branch(CloneRequest {
                clone_url: record.clone_url.clone(),
                dest: dest.clone(),
                branch: Some(plan.base.clone()),
                token: plan.token.clone(),
            })
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        self.deps
            .git
            .create_branch(dest.clone(), plan.branch.clone())
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        Ok(dest)
    }

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
        Ok(ctx.recording_provider(inner))
    }

    /// Run the agent over the checkout with the full transcript as context.
    async fn run_agent(
        &self,
        ctx: &JobContext,
        state: &TurnState<'_>,
        profile_id: Uuid,
    ) -> Result<String, JobError> {
        let provider = self.provider(ctx, profile_id).await?;
        let messages = self
            .deps
            .tasks
            .messages(state.task.id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let request = AiRequest::new(build_prompt(&messages))
            .with_system(agent_system(&state.record.full_name))
            .with_working_dir(state.checkout.clone());
        let response = provider
            .complete(request)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        Ok(response.text)
    }

    async fn add_agent_message(&self, ctx: &JobContext, task_id: Uuid, content: &str) {
        let _ = self
            .deps
            .tasks
            .add_message(NewMessage {
                task_id,
                role: role::AGENT.to_string(),
                content: content.to_string(),
                execution_id: Some(ctx.execution_id),
            })
            .await;
    }

    /// Commit; on no-change report back, else push and open/update the PR.
    async fn publish(
        &self,
        ctx: &JobContext,
        state: &TurnState<'_>,
        summary: &str,
    ) -> Result<JobOutcome, JobError> {
        let committed = self
            .deps
            .git
            .commit_all(CommitRequest {
                checkout: state.checkout.clone(),
                message: format!("AI Agent: {}", state.task.title),
                author_name: "PlatIQ Agent".to_string(),
                author_email: "agent@platiq.local".to_string(),
            })
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        if !committed {
            return self.no_changes(ctx, state, summary).await;
        }
        self.deps
            .git
            .push_branch(PushRequest {
                checkout: state.checkout.clone(),
                branch: state.target.branch_name.clone(),
                token: state.token.clone(),
            })
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        self.open_pr(ctx, state, summary).await
    }

    async fn open_pr(
        &self,
        ctx: &JobContext,
        state: &TurnState<'_>,
        summary: &str,
    ) -> Result<JobOutcome, JobError> {
        let pr = self
            .deps
            .accounts
            .open_pull_request(
                &state.account,
                PullRequestSpec {
                    repo_full_name: state.record.full_name.clone(),
                    head_branch: state.target.branch_name.clone(),
                    base_branch: state.base.clone(),
                    title: state.task.title.clone(),
                    body: pr_body(summary),
                },
            )
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let _ = self
            .deps
            .tasks
            .update_target_status(state.target.id, status::PR_OPEN, Some(pr.url.clone()))
            .await;
        let _ = self
            .deps
            .tasks
            .update_status(state.task.id, status::PR_OPEN, Some(pr.url.clone()))
            .await;
        let line = format!("**{}** → {}\n\n{summary}", state.record.full_name, pr.url);
        self.add_agent_message(ctx, state.task.id, &line).await;
        ctx.merge_metadata(&json!({ "pr_url": pr.url, "pr_number": pr.number })).await;
        Ok(JobOutcome::completed(json!({
            "status": status::PR_OPEN,
            "pr_url": pr.url,
            "branch": state.target.branch_name,
        })))
    }

    async fn no_changes(
        &self,
        ctx: &JobContext,
        state: &TurnState<'_>,
        summary: &str,
    ) -> Result<JobOutcome, JobError> {
        let msg = format!(
            "{summary}\n\n(No file changes were produced. Use a Claude CLI agent profile so the \
             agent can edit files.)"
        );
        self.add_agent_message(ctx, state.task.id, &msg).await;
        let _ = self
            .deps
            .tasks
            .update_target_status(state.target.id, status::AWAITING_INPUT, None)
            .await;
        let _ = self
            .deps
            .tasks
            .update_status(state.task.id, status::AWAITING_INPUT, None)
            .await;
        Ok(JobOutcome::completed(json!({ "status": status::AWAITING_INPUT, "changes": false })))
    }

    /// Resolve the checkout, run the agent, and publish the result for one
    /// repository target.
    async fn run_turn(
        &self,
        ctx: &JobContext,
        task: &AgentTask,
        target: &AgentTaskTarget,
        input: &TurnInput,
    ) -> Result<JobOutcome, JobError> {
        let record = self
            .deps
            .repositories
            .get(target.repository_id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let _ = self.deps.tasks.update_target_status(target.id, status::RUNNING, None).await;
        let _ = self.deps.tasks.update_status(task.id, status::RUNNING, None).await;
        let (account, token) = self.resolve_account(&record).await?;
        let plan = CheckoutPlan {
            base: Self::base_branch(&record),
            branch: target.branch_name.clone(),
            token: token.clone(),
        };
        let checkout = self.prepare_checkout(ctx, &record, &plan).await?;
        let state = TurnState {
            task,
            target,
            record: &record,
            account,
            token,
            base: plan.base,
            checkout,
        };
        let summary = self.run_agent(ctx, &state, input.ai_profile_id).await?;
        self.publish(ctx, &state, &summary).await
    }
}

#[async_trait]
impl JobType for AgentTaskJob {
    fn id(&self) -> &str {
        JOB_TYPE
    }

    fn description(&self) -> &str {
        "Run an agentic AI session over an application's repository: edit files on a dedicated \
         branch, commit, push, and open a pull request. Serialised per repository with a lock."
    }

    async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError> {
        let input = Self::parse_input(&ctx)?;
        let task = self
            .deps
            .tasks
            .get(input.task_id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let target = self
            .deps
            .tasks
            .get_target(input.target_id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let record = self
            .deps
            .repositories
            .get(target.repository_id)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;

        // Serialise per repository: a concurrent turn on the same repo reschedules.
        let key = lock_keys::repository(&record.full_name);
        let lease = match self.deps.lock.acquire(&key, LOCK_TTL).await {
            Ok(Some(lease)) => lease,
            Ok(None) => {
                ctx.log("repository is busy — rescheduling").await;
                return Err(JobError::CannotRun { retry_at: None });
            }
            Err(e) => return Err(JobError::Failed(format!("repository lock error: {e}"))),
        };
        let outcome = self.run_turn(&ctx, &task, &target, &input).await;
        let _ = self.deps.lock.release(&lease).await;
        if matches!(outcome, Err(JobError::Failed(_))) {
            let _ = self.deps.tasks.update_target_status(target.id, status::FAILED, None).await;
            let _ = self.deps.tasks.update_status(task.id, status::FAILED, None).await;
        }
        outcome
    }
}

/// Build the agent prompt from the transcript (its last entry is the latest
/// user instruction).
fn build_prompt(messages: &[AgentTaskMessage]) -> String {
    let mut out = String::new();
    for m in messages {
        out.push_str(&format!("{}: {}\n\n", m.role, m.content));
    }
    out.push_str("Make the requested changes to the repository now, then summarise what you did.");
    out
}

fn agent_system(full_name: &str) -> String {
    format!(
        "You are an autonomous software engineer working on the repository '{full_name}', checked \
         out in your working directory on a dedicated branch. Make the requested code changes by \
         editing files directly. Keep changes focused and correct. Summarise what you changed."
    )
}

fn pr_body(summary: &str) -> String {
    format!("Automated change by the PlatIQ AI Agent.\n\n{summary}")
}

/// Find the singleton `application-agent-task` job, creating it if absent. The
/// `max_concurrency` (config) lets agent turns run in parallel up to that many at
/// once (per-repo locks still serialise same-repo work).
pub async fn ensure_job(jobs: &dyn JobRepository, max_concurrency: u32) -> Result<Uuid, AppError> {
    let existing = jobs.list().await?;
    if let Some(job) = existing.iter().find(|j| j.job_type == JOB_TYPE) {
        return Ok(job.id);
    }
    let created = jobs
        .create(JobInput {
            job_type: JOB_TYPE.to_string(),
            name: "AI Agent tasks".to_string(),
            trigger_type: TriggerType::Manual,
            cron_expr: None,
            config: json!({ "max_concurrency": max_concurrency.max(1) }),
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
    use crate::agent_tasks::repository::MockAgentTaskRepository;
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
    use chrono::Utc;
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
        }
    }

    fn task() -> AgentTask {
        AgentTask {
            id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            repository_id: Uuid::new_v4(),
            title: "Add a health endpoint".into(),
            status: status::RUNNING.into(),
            branch_name: "agent/abc".into(),
            pr_url: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn message(role: &str, content: &str) -> AgentTaskMessage {
        AgentTaskMessage {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            role: role.into(),
            content: content.into(),
            execution_id: None,
            created_at: Utc::now(),
        }
    }

    fn deps(
        tasks: MockAgentTaskRepository,
        repos: MockRepoRecordRepository,
        lock: MockDistributedLock,
    ) -> AgentTaskDeps {
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
        AgentTaskDeps {
            tasks: Arc::new(tasks),
            accounts: AccountService::new(
                Arc::new(MockRepositoryAccountRepository::new()),
                provider_deps,
            ),
            repositories: Arc::new(repos),
            git: Arc::new(MockGitClient::new()),
            workspace: Workspace::new(Arc::new(MockFileSystem::new()), "/w".into()),
            ai: AiProfileService::new(Arc::new(MockAiProfileRepository::new()), ai_deps.clone()),
            ai_deps,
            lock: Arc::new(lock),
        }
    }

    fn ctx_with(params: Value) -> JobContext {
        JobContext {
            execution_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            job_name: "agent".into(),
            config: json!({}),
            params,
            state: Value::Null,
            log: Arc::new(MockLogSink::new()),
            executions: Arc::new(MockJobExecutionRepository::new()),
            clock: Arc::new(crate::jobs::clock::SystemClock),
        }
    }

    #[test]
    fn parse_input_rejects_missing_fields() {
        let ctx = ctx_with(json!({ "task_id": Uuid::new_v4() }));
        assert!(AgentTaskJob::parse_input(&ctx).is_err());
    }

    #[test]
    fn base_branch_uses_default_then_main() {
        assert_eq!(AgentTaskJob::base_branch(&record()), "develop");
        let mut bare = record();
        bare.default_branch = None;
        assert_eq!(AgentTaskJob::base_branch(&bare), "main");
    }

    #[test]
    fn build_prompt_includes_transcript() {
        let msgs = vec![message("user", "add tests"), message("agent", "done")];
        let prompt = build_prompt(&msgs);
        assert!(prompt.contains("user: add tests"));
        assert!(prompt.contains("agent: done"));
        assert!(prompt.contains("Make the requested changes"));
    }

    #[test]
    fn pr_body_wraps_summary() {
        assert!(pr_body("changed x").contains("changed x"));
        assert!(pr_body("changed x").contains("PlatIQ AI Agent"));
    }

    fn target() -> AgentTaskTarget {
        AgentTaskTarget {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            repository_id: Uuid::new_v4(),
            branch_name: "agent/abc".into(),
            status: status::PENDING.into(),
            pr_url: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn busy_repository_reschedules_without_failing() {
        let mut tasks = MockAgentTaskRepository::new();
        let fixture = task();
        let task_id = fixture.id;
        tasks.expect_get().returning(move |_| Ok(fixture.clone()));
        tasks.expect_get_target().returning(|_| Ok(target()));
        let mut repos = MockRepoRecordRepository::new();
        repos.expect_get().returning(|_| Ok(record()));
        let mut lock = MockDistributedLock::new();
        lock.expect_acquire().returning(|_, _| Ok(None));
        let job = AgentTaskJob::new(deps(tasks, repos, lock));

        let mut log = MockLogSink::new();
        log.expect_append().returning(|_, _| Ok(()));
        let ctx = JobContext {
            log: Arc::new(log),
            ..ctx_with(json!({
                "task_id": task_id,
                "target_id": Uuid::new_v4(),
                "message": "add a feature",
                "ai_profile_id": Uuid::new_v4(),
            }))
        };
        assert!(matches!(job.run(ctx).await, Err(JobError::CannotRun { .. })));
    }

    // --- Full-turn happy path (commit → push → PR) and the no-change path ----

    fn recording_tasks(statuses: std::sync::Arc<std::sync::Mutex<Vec<String>>>) -> MockAgentTaskRepository {
        let mut tasks = MockAgentTaskRepository::new();
        tasks.expect_get().returning(|_| Ok(task()));
        tasks.expect_get_target().returning(|_| Ok(target()));
        tasks.expect_update_target_status().returning(|_, _, _| Ok(()));
        tasks
            .expect_messages()
            .returning(|_| Ok(vec![message("user", "add a health endpoint")]));
        tasks.expect_update_status().returning(move |_, status, _| {
            statuses.lock().unwrap().push(status.to_string());
            Ok(())
        });
        tasks.expect_add_message().returning(|input| {
            Ok(AgentTaskMessage {
                id: Uuid::new_v4(),
                task_id: input.task_id,
                role: input.role,
                content: input.content,
                execution_id: input.execution_id,
                created_at: Utc::now(),
            })
        });
        tasks
    }

    /// AccountService whose GitHub provider returns a fixed PR over a mock HTTP.
    fn github_accounts() -> AccountService {
        use crate::accounts::model::{AuthType, ProviderType, RepositoryAccount, SelectionMode};
        let mut acct = MockRepositoryAccountRepository::new();
        acct.expect_get().returning(|id| {
            Ok(RepositoryAccount {
                id,
                name: "gh".into(),
                provider_type: ProviderType::Github,
                auth_type: AuthType::Token,
                base_url: None,
                credentials_enc: None,
                selection_mode: SelectionMode::All,
                selection_value: None,
                enabled: true,
            })
        });
        let mut http = MockHttpClient::new();
        http.expect_send().returning(|_| {
            Ok(crate::httpclient::HttpResponse::new(
                201,
                r#"{"number":7,"html_url":"https://github.com/org/api/pull/7","state":"open"}"#,
            ))
        });
        AccountService::new(
            Arc::new(acct),
            ProviderDeps {
                http: Arc::new(http),
                fs: Arc::new(MockFileSystem::new()),
                encryptor: Arc::new(MockEncryptor::new()),
            },
        )
    }

    fn cli_ai_service() -> AiProfileService {
        use crate::ai::AiProviderType;
        use crate::ai::model::AiProfile;
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
        AiProfileService::new(Arc::new(ai_repo), throwaway_ai_deps())
    }

    fn throwaway_ai_deps() -> AiProviderDeps {
        AiProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            runner: Arc::new(MockCommandRunner::new()),
            encryptor: Arc::new(MockEncryptor::new()),
        }
    }

    /// AI deps whose command runner returns a Claude-CLI-style JSON result.
    fn cli_ai_deps() -> AiProviderDeps {
        let mut runner = MockCommandRunner::new();
        runner.expect_run().returning(|_| {
            Ok(crate::process::CommandOutput {
                status: 0,
                stdout: r#"{"result":"Edited 2 files"}"#.into(),
                stderr: String::new(),
            })
        });
        AiProviderDeps {
            http: Arc::new(MockHttpClient::new()),
            runner: Arc::new(runner),
            encryptor: Arc::new(MockEncryptor::new()),
        }
    }

    fn succeeding_git(committed: bool) -> MockGitClient {
        use crate::git::CheckoutInfo;
        let mut git = MockGitClient::new();
        git.expect_sync_branch()
            .returning(|req| Ok(CheckoutInfo { commit_sha: "sha".into(), path: req.dest }));
        git.expect_create_branch().returning(|_, _| Ok(()));
        git.expect_commit_all().returning(move |_| Ok(committed));
        git.expect_push_branch().returning(|_| Ok(()));
        git
    }

    fn full_deps(committed: bool, statuses: std::sync::Arc<std::sync::Mutex<Vec<String>>>) -> AgentTaskDeps {
        let mut ws_fs = MockFileSystem::new();
        ws_fs.expect_create_dir_all().returning(|_| Ok(()));
        let mut lock = MockDistributedLock::new();
        lock.expect_acquire().returning(|key, _| {
            Ok(Some(crate::locks::Lease {
                key: key.to_string(),
                token: "tok".into(),
                expires_at: Utc::now(),
            }))
        });
        lock.expect_release().returning(|_| Ok(()));
        let mut repos = MockRepoRecordRepository::new();
        repos.expect_get().returning(|_| Ok(record()));
        AgentTaskDeps {
            tasks: Arc::new(recording_tasks(statuses)),
            accounts: github_accounts(),
            repositories: Arc::new(repos),
            git: Arc::new(succeeding_git(committed)),
            workspace: Workspace::new(Arc::new(ws_fs), "/w".into()),
            ai: cli_ai_service(),
            ai_deps: cli_ai_deps(),
            lock: Arc::new(lock),
        }
    }

    fn recording_ctx(task_id: Uuid) -> JobContext {
        let mut log = MockLogSink::new();
        log.expect_append().returning(|_, _| Ok(()));
        let mut execs = MockJobExecutionRepository::new();
        execs.expect_merge_metadata().returning(|_, _| Ok(()));
        JobContext {
            log: Arc::new(log),
            executions: Arc::new(execs),
            ..ctx_with(json!({
                "task_id": task_id,
                "target_id": Uuid::new_v4(),
                "message": "add a health endpoint",
                "ai_profile_id": Uuid::new_v4(),
            }))
        }
    }

    #[tokio::test]
    async fn full_turn_commits_pushes_and_opens_pr() {
        let statuses = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let job = AgentTaskJob::new(full_deps(true, statuses.clone()));
        let outcome = job.run(recording_ctx(Uuid::new_v4())).await;
        assert!(outcome.is_ok(), "happy turn succeeds: {outcome:?}");
        // The PR-open status transition was recorded.
        assert!(statuses.lock().unwrap().iter().any(|s| s == status::PR_OPEN));
    }

    #[tokio::test]
    async fn no_change_turn_sets_awaiting_input() {
        let statuses = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let job = AgentTaskJob::new(full_deps(false, statuses.clone()));
        let outcome = job.run(recording_ctx(Uuid::new_v4())).await;
        assert!(outcome.is_ok());
        // Nothing was committed → the task awaits more input (no PR).
        let recorded = statuses.lock().unwrap();
        assert!(recorded.iter().any(|s| s == status::AWAITING_INPUT));
        assert!(!recorded.iter().any(|s| s == status::PR_OPEN));
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
        let created = ensure_job(&jobs, 4).await.unwrap();

        let mut jobs2 = MockJobRepository::new();
        jobs2.expect_list().returning(move || {
            Ok(vec![Job {
                id: created,
                job_type: JOB_TYPE.into(),
                name: "AI Agent tasks".into(),
                trigger_type: TriggerType::Manual,
                cron_expr: None,
                config: json!({}),
                enabled: true,
                next_run_at: None,
            }])
        });
        assert_eq!(ensure_job(&jobs2, 4).await.unwrap(), created);
    }
}
