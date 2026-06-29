# Milestone 24 — PR watcher (polling): comments, conflicts & failed checks

## Goal

Keep agent-task PRs (M22/M23) healthy **without webhooks**, via a scheduled
poller. A new built-in job, **`pr-watcher`**, runs **every minute** (leader-
elected, one instance), finds open tasks that have an associated PR, and:

- if the PR is **merged or closed**, finishes the task (and its target);
- if the PR is **open**, checks for **new review comments**, **merge conflicts**,
  and **failed CI checks/Actions**, and — when any of those needs attention —
  dispatches the LLM agent to fix them and **post a PR comment** summarising the
  fixes.

This is the **polling** solution: simpler and works on any provider/deployment
(no inbound webhook required), at the cost of up-to-a-minute latency and more API
calls than the webhook approach (M25). The two share one reconciliation core.

## Scope

- A `pr-watcher` cron job (1-minute schedule) that reconciles open PR-bearing
  tasks/targets.
- Provider extensions to read PR merge state, comments, and check/Action results,
  and to post a comment.
- A git operation to surface merge conflicts as editable files for the agent.
- A shared **PR reconciliation service** that decides what (if anything) needs
  fixing and enqueues an agent fix-turn — reused verbatim by webhooks in M25.
- Per-task high-water marks so the same comment/check/conflict isn't fixed twice.

## Deliverables

### Provider extensions

Extend `RepositoryProvider` (GitHub impl real; `Unsupported` default elsewhere):

```rust
/// Rich PR status for reconciliation.
async fn pull_request_status(&self, repo: &str, number: u64) -> Result<PrStatus, ProviderError>;
/// Review + issue comments created after `since` (None = all).
async fn pull_request_comments(&self, repo: &str, number: u64, since: Option<DateTime<Utc>>)
    -> Result<Vec<PrComment>, ProviderError>;
/// Check-runs / combined commit status for the PR head.
async fn pull_request_checks(&self, repo: &str, head_sha: &str) -> Result<Vec<PrCheck>, ProviderError>;
/// Failed check output/logs (best-effort summary the LLM can act on).
async fn check_output(&self, repo: &str, check: &PrCheck) -> Result<String, ProviderError>;
/// Post a comment on the PR.
async fn post_pull_request_comment(&self, repo: &str, number: u64, body: &str) -> Result<(), ProviderError>;
```

`PrStatus { state: open|closed|merged, mergeable: Option<bool>, mergeable_state:
String, head_sha: String }`; `PrComment { id, author, body, created_at }`;
`PrCheck { name, status, conclusion, details_url }`. GitHub maps these to the
PRs API (`mergeable`/`mergeable_state`), the review/issue comments APIs, and
check-runs / combined status; failed-check output comes from check-run
annotations or workflow-job logs.

### Git: surface merge conflicts

Extend `GitClient` so the agent can resolve conflicts in the working tree:

```rust
/// Fetch and merge `base` into the checked-out branch. Returns whether it
/// conflicted and the conflicted paths (left with conflict markers for editing).
async fn merge_base(&self, request: MergeRequest) -> Result<MergeOutcome, GitError>;
```

`MergeOutcome { conflicted: bool, files: Vec<String> }`. The fix-turn merges the
base branch in, the agent edits the conflicted files, then `commit_all` +
`push_branch` (M22) update the PR.

### Reconciliation service (shared with M25)

A provider/poller-agnostic `PrReconciler` that, given a task target with an open
PR, decides the action — reused by both this job and the webhook handler:

1. `pull_request_status`:
   - **merged** → mark target `merged`, parent task `completed` when all targets
     are merged/closed; add an agent message "PR merged".
   - **closed** (not merged) → mark target `closed`; finish the task if no open
     targets remain.
   - **open** → evaluate the three signals below.
2. **Signals** (only act on *new* ones, tracked via per-target high-water marks
   in `agent_tasks`/target metadata: `last_comment_id`, `last_checked_sha`):
   - **New review comments** since `last_comment_id` (excluding the agent's own).
   - **Merge conflict** (`mergeable == false` / `mergeable_state == "dirty"`).
   - **Failed checks** on `head_sha` (any `conclusion == failure`).
3. If any signal fires, build a fix instruction (the comment text / "resolve
   conflicts with `base`" / the failed-check output) and **enqueue an agent
   fix-turn** for that target (M22 turn machinery, per-repo lock). The turn
   checks out the PR branch, applies the fix, pushes, and **posts a PR comment**
   listing what it changed. Update the high-water marks so the same signal isn't
   re-processed.

Acting on the heavy fix goes through the existing per-repo-locked agent job; the
watcher itself only polls and enqueues, so it stays cheap.

### The job

`src/pr_watcher/` with `PrWatcherJob` implementing `JobType`
(`JOB_TYPE = "pr-watcher"`):

- `ensure_job` seeds a singleton job with a **cron of `* * * * *`** (every
  minute) and `enabled: true`; the `CronScheduler` + leader-elected
  `JobController` (M13) run exactly one instance.
- `run(ctx)` lists open PR-bearing targets (`AgentTaskRepository`), and for each
  calls `PrReconciler`. It is read-mostly and idempotent — a target with no new
  signal is a no-op.
- Register in `AppState::build`; seed in `main`.

## Tasks

- [ ] Provider PR-introspection methods (status/comments/checks/output/comment)
      + GitHub impl + mocks; `Unsupported` defaults.
- [ ] `GitClient::merge_base` (conflict surfacing) + impl/mock.
- [ ] `PrReconciler` (merged/closed/finish; comment/conflict/failed-check →
      fix-turn) with high-water-mark dedup.
- [ ] `PrWatcherJob` (`* * * * *`, leader-elected) + `ensure_job`; register/seed.
- [ ] Fix-turn instruction builders (comment / conflict / failed-check).
- [ ] Unit tests (mocked provider/git/lock/repo/clock): merged → finished;
      closed → finished; new comment / conflict / failed check each enqueues a
      single fix-turn; already-seen signals are no-ops; the agent posts a comment.

## Acceptance criteria

- With no webhooks configured, an open agent-task PR that receives a review
  comment, develops a merge conflict, or fails CI is fixed by the agent within
  ~a minute, and the agent posts a comment describing the fix.
- A PR that is merged or closed finishes its task/target automatically.
- Each signal is acted on once (high-water marks prevent repeated fixes).
- Exactly one instance runs the watcher (leader election); the reconciliation
  core is unit-tested with mocked dependencies.

## Dependencies

Milestones 22 (agent tasks + PRs), 23 (task targets), 13/06 (cron scheduling +
leader-elected controller), 12 (per-repo locks).

## Out of scope

Webhook delivery (M25 — same `PrReconciler`), auto-merging PRs, and approving
reviews. The watcher fixes and comments; a human still merges.
