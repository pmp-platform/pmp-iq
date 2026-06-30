//! The `pr-watcher` job (M24): a polling reconciler for agent-task PRs. Runs
//! every minute (leader-elected via the cron scheduler), finishing tasks whose
//! PR merged/closed and dispatching the agent to fix new review comments, merge
//! conflicts, or failed CI checks — posting a PR comment when it acts.

use crate::accounts::{AccountService, RepositoryAccount};
use crate::agent_tasks::model::status;
use crate::agent_tasks::repository::AgentTaskRepository;
use crate::agent_tasks::{self, AgentTaskTarget, NewMessage};
use crate::ai::AiProfileService;
use crate::error::AppError;
use crate::jobs::model::{JobInput, TriggerType};
use crate::jobs::repository::{JobExecutionRepository, JobRepository};
use crate::jobs::{JobContext, JobError, JobOutcome, JobType};
use crate::repositories::{RepoRecord, RepoRecordRepository};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub const JOB_TYPE: &str = "pr-watcher";
/// Prefix on the agent's own PR comments, used to dedup: when the latest comment
/// is ours, the watcher has already acted on the current state.
const AGENT_MARKER: &str = "🤖 pmp-iq:";

/// Dependencies for the watcher (bundled to bound parameter count).
#[derive(Clone)]
pub struct PrWatcherDeps {
    pub tasks: Arc<dyn AgentTaskRepository>,
    pub repositories: Arc<dyn RepoRecordRepository>,
    pub accounts: AccountService,
    pub ai: AiProfileService,
    pub jobs: Arc<dyn JobRepository>,
    pub executions: Arc<dyn JobExecutionRepository>,
    pub agent_max_concurrency: u32,
}

/// Resolved context for reconciling one open-PR target.
struct Reconcile<'a> {
    target: &'a AgentTaskTarget,
    record: RepoRecord,
    account: RepositoryAccount,
    number: u64,
}

/// Polls open agent-task PRs and reconciles them.
pub struct PrWatcherJob {
    deps: PrWatcherDeps,
}

impl PrWatcherJob {
    pub fn new(deps: PrWatcherDeps) -> Self {
        Self { deps }
    }

    async fn default_profile(&self) -> Option<Uuid> {
        let profiles = self.deps.ai.list().await.ok()?;
        profiles.iter().find(|p| p.enabled).or_else(|| profiles.first()).map(|p| p.id)
    }

    async fn add_user_message(&self, task_id: Uuid, content: &str) {
        let _ = self
            .deps
            .tasks
            .add_message(NewMessage {
                task_id,
                role: "user".to_string(),
                content: content.to_string(),
                execution_id: None,
            })
            .await;
    }

    /// Resolve the repo + account + PR number for an open-PR target.
    async fn resolve<'a>(&self, target: &'a AgentTaskTarget) -> Option<Reconcile<'a>> {
        let number = parse_pr_number(target.pr_url.as_deref()?)?;
        let record = self.deps.repositories.get(target.repository_id).await.ok()?;
        let account = self.deps.accounts.get(record.account_id).await.ok()?;
        Some(Reconcile { target, record, account, number })
    }

    /// Mark a target finished (merged/closed) and recompute the parent task.
    async fn finish(&self, rc: &Reconcile<'_>, state: &str, note: &str) {
        let _ = self.deps.tasks.update_target_status(rc.target.id, state, None).await;
        self.add_user_message(rc.target.task_id, note).await;
        self.recompute_task(rc.target.task_id).await;
    }

    /// If every target is merged/closed, settle the parent task accordingly.
    async fn recompute_task(&self, task_id: Uuid) {
        let targets = self.deps.tasks.list_targets(task_id).await.unwrap_or_default();
        let all_done = targets
            .iter()
            .all(|t| t.status == status::MERGED || t.status == status::CLOSED);
        if targets.is_empty() || !all_done {
            return;
        }
        let any_merged = targets.iter().any(|t| t.status == status::MERGED);
        let state = if any_merged { status::MERGED } else { status::CLOSED };
        let _ = self.deps.tasks.update_status(task_id, state, None).await;
    }

    /// Reconcile one open-PR target: finish merged/closed PRs, or detect new
    /// comments / conflicts / failed checks and enqueue a continue-branch fix
    /// turn (posting a PR comment). Returns whether it acted.
    async fn reconcile_one(&self, rc: &Reconcile<'_>, agent_job_id: Uuid, profile_id: Uuid) -> bool {
        let repo = &rc.record.full_name;
        let status = match self.deps.accounts.pr_status(&rc.account, repo, rc.number).await {
            Ok(s) => s,
            Err(_) => return false, // unsupported provider / transient → skip
        };
        if status.state == "merged" {
            self.finish(rc, status::MERGED, &format!("**{repo}** PR was merged. ✅")).await;
            return true;
        }
        if status.state == "closed" {
            self.finish(rc, status::CLOSED, &format!("**{repo}** PR was closed without merging.")).await;
            return true;
        }
        let comments = self.deps.accounts.pr_comments(&rc.account, repo, rc.number).await.unwrap_or_default();
        // Dedup: if our marker is the latest comment we've already acted.
        if comments.last().map(|c| c.body.starts_with(AGENT_MARKER)).unwrap_or(false) {
            return false;
        }
        let checks = self.deps.accounts.pr_checks(&rc.account, repo, &status.head_sha).await.unwrap_or_default();
        let conflict = status.mergeable == Some(false);
        let failed: Vec<String> = checks
            .iter()
            .filter(|c| c.conclusion.as_deref() == Some("failure"))
            .map(|c| c.name.clone())
            .collect();
        let human = comments.last().map(|c| c.body.clone());
        if !conflict && failed.is_empty() && human.is_none() {
            return false;
        }
        let base = rc.record.default_branch.clone().unwrap_or_else(|| "main".to_string());
        let instruction = build_fix_instruction(repo, &base, conflict, &failed, human.as_deref());
        let note = format!("{AGENT_MARKER} addressing the review feedback / CI / conflicts; a fix is on the way.");
        let _ = self.deps.accounts.post_pr_comment(&rc.account, repo, rc.number, &note).await;
        self.add_user_message(rc.target.task_id, &instruction).await;
        let params = json!({
            "task_id": rc.target.task_id,
            "target_id": rc.target.id,
            "message": instruction,
            "ai_profile_id": profile_id,
            "continue_branch": true,
        });
        // Enqueue as a queued execution; the controller's dispatcher (M27) runs it.
        self.deps.executions.create(agent_job_id, "pr-watcher", &params).await.is_ok()
    }
}

#[async_trait]
impl JobType for PrWatcherJob {
    fn id(&self) -> &str {
        JOB_TYPE
    }

    fn description(&self) -> &str {
        "Poll open AI-Agent pull requests every minute: finish merged/closed ones, and dispatch \
         the agent to fix new comments, merge conflicts, or failed CI checks."
    }

    async fn run(&self, ctx: JobContext) -> Result<JobOutcome, JobError> {
        let agent_job_id = agent_tasks::ensure_job(self.deps.jobs.as_ref(), self.deps.agent_max_concurrency)
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let Some(profile_id) = self.default_profile().await else {
            return Ok(JobOutcome::completed(json!({ "reconciled": 0, "note": "no AI profile" })));
        };
        let targets = self
            .deps
            .tasks
            .list_open_pr_targets()
            .await
            .map_err(|e| JobError::Failed(e.to_string()))?;
        let mut acted = 0usize;
        for target in &targets {
            if let Some(rc) = self.resolve(target).await {
                if self.reconcile_one(&rc, agent_job_id, profile_id).await {
                    acted += 1;
                }
            }
        }
        if acted > 0 {
            ctx.log(&format!("reconciled {} PR(s); acted on {acted}", targets.len())).await;
        }
        Ok(JobOutcome::completed(json!({ "reconciled": targets.len(), "acted": acted })))
    }
}

/// Parse the PR number from a PR URL (`…/pull/7` or `…/merge_requests/7`).
fn parse_pr_number(url: &str) -> Option<u64> {
    url.trim_end_matches('/').rsplit('/').next()?.parse().ok()
}

fn build_fix_instruction(
    repo: &str,
    base: &str,
    conflict: bool,
    failed: &[String],
    human: Option<&str>,
) -> String {
    let mut s = format!(
        "You are continuing work on the open pull request for repository '{repo}'. Your working \
         directory is checked out on the PR branch. "
    );
    if conflict {
        s.push_str(&format!(
            "The PR has merge conflicts with the base branch '{base}'. Merge or rebase \
             'origin/{base}' into the branch, resolve all conflicts, and commit the result. "
        ));
    }
    if !failed.is_empty() {
        s.push_str(&format!(
            "These CI checks are failing — investigate and fix them: {}. ",
            failed.join(", ")
        ));
    }
    if let Some(comment) = human {
        s.push_str(&format!("Address this review comment:\n\n{comment}\n\n"));
    }
    s.push_str("Make the necessary code changes, then summarise what you did.");
    s
}

/// Find the singleton `pr-watcher` cron job, creating it (every minute) if absent.
pub async fn ensure_job(jobs: &dyn JobRepository) -> Result<Uuid, AppError> {
    let existing = jobs.list().await?;
    if let Some(job) = existing.iter().find(|j| j.job_type == JOB_TYPE) {
        return Ok(job.id);
    }
    let created = jobs
        .create(JobInput {
            job_type: JOB_TYPE.to_string(),
            name: "PR watcher".to_string(),
            trigger_type: TriggerType::Cron,
            cron_expr: Some("* * * * *".to_string()),
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
    fn parses_pr_number_from_urls() {
        assert_eq!(parse_pr_number("https://github.com/org/api/pull/7"), Some(7));
        assert_eq!(parse_pr_number("https://gitlab.com/g/p/-/merge_requests/42"), Some(42));
        assert_eq!(parse_pr_number("https://github.com/org/api/pull/"), None);
        assert_eq!(parse_pr_number("not-a-url"), None);
    }

    #[test]
    fn fix_instruction_mentions_each_signal() {
        let i = build_fix_instruction("org/api", "main", true, &["build".into()], Some("please rename x"));
        assert!(i.contains("merge conflicts"));
        assert!(i.contains("origin/main"));
        assert!(i.contains("build"));
        assert!(i.contains("please rename x"));
    }

    #[test]
    fn fix_instruction_omits_absent_signals() {
        let i = build_fix_instruction("org/api", "main", false, &[], None);
        assert!(!i.contains("merge conflicts"));
        assert!(!i.contains("CI checks"));
        assert!(i.contains("summarise"));
    }
}
