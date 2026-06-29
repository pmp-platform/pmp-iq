# Milestone 22 — Application "AI Agent" tab — change tasks & PRs

## Goal

Add an **"AI Agent"** tab to the application detail page where a user can create
**tasks** against the application's repository. Each task is a **session** with an
agentic AI (the Claude CLI provider, which can read and edit the checkout): the
user describes a change in natural language, the agent edits the repo on a
dedicated branch, commits, pushes, and opens a **pull request**. The user can
send **follow-up messages** in the same session to refine the change; the PR is
updated on the same branch.

Where M15 (Q&A) is read-only single-shot, this milestone is the read-write,
multi-turn agentic flow that produces real PRs.

## Scope

- An `application-agent-task` job type that runs one agent turn over a writable
  checkout: branch → agent edits → commit → push → open/update PR.
- Persistence for tasks and their message transcript (the "session").
- Git push + provider PR creation behind the existing traits (mockable).
- Routes to list/create tasks, post follow-ups, and poll status.
- An "AI Agent" tab in the application detail page: task list, composer, and a
  chat-style transcript with the live PR link.

## Deliverables

### Persistence

Dual-engine dbmate migrations (Postgres + SQLite), up-only:

```sql
-- migrate:up
CREATE TABLE agent_tasks (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    repository_id  UUID NOT NULL,
    title          TEXT NOT NULL,
    status         TEXT NOT NULL,        -- draft|running|awaiting_input|pr_open|failed
    branch_name    TEXT NOT NULL,        -- agent/<task-id>
    pr_url         TEXT,
    created_at     TIMESTAMPTZ NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL
);
CREATE TABLE agent_task_messages (
    id           UUID PRIMARY KEY,
    task_id      UUID NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    role         TEXT NOT NULL,          -- user|agent
    content      TEXT NOT NULL,
    execution_id UUID,                   -- the job execution that produced it
    created_at   TIMESTAMPTZ NOT NULL
);
-- migrate:down
```

A dual-engine `AgentTaskRepository` trait (Pg + SQLite impls, picked in
`store.rs`): create/get/list tasks for an application, append messages, update
status/`pr_url`. Mocked in unit tests.

### Git: commit & push

Extend the `GitClient` trait (`src/git.rs`) — it already has
`clone_or_update`/`sync_branch`/`CloneRequest`; add the write side, kept on the
blocking thread like the rest of `git2`:

- `create_branch(checkout, branch, base)` — branch off the default branch head.
- `commit_all(checkout, message, author)` — stage all changes and commit; report
  whether anything was committed (no-op when the agent made no edits).
- `push_branch(PushRequest { checkout, branch, remote_url, token })` — push with
  token credentials. New trait methods get mock impls for unit tests.

### Provider: pull requests

Extend the `RepositoryProvider` trait (`src/accounts/providers/`) — it already
has `list_repositories`/`list_members`; add PR operations:

```rust
async fn open_pull_request(&self, req: PullRequestSpec) -> Result<PullRequest, ProviderError>;
async fn get_pull_request(&self, repo_full_name: &str, number: u64)
    -> Result<PullRequest, ProviderError>;
```

`PullRequestSpec { repo_full_name, head_branch, base_branch, title, body }`.
Implemented for **GitHub** via the PRs REST API over the existing `HttpClient`
(reuse the token from the account / M21 GitHub App); a `ProviderError::Unsupported`
default for providers that can't open PRs (GitLab/local), surfaced clearly in the
UI. Re-running a task with an existing open PR on the same branch updates it
rather than opening a duplicate (find-or-create by head branch).

### The job (`src/agent_tasks/`)

A new module mirroring `src/llm_request/`, with `AgentTaskJob` implementing
`JobType` and an `AgentTaskDeps` parameter struct (repos, git, provider factory,
AI service, workspace, lock, clock — to stay within the ≤4-param rule). Each
execution runs **one turn**; `params` carry `{ task_id, application_id, message,
ai_profile_id }`. `run(ctx)`:

1. Acquire the per-repository `DistributedLock` (M12); on contention return
   `JobError::CannotRun { retry_at }` so M13 reschedules it (no two turns touch
   one repo at once). Refresh the lease during the run; release on exit.
2. Clone/`sync_branch` into the per-job workspace (M13); create or checkout the
   task's `agent/<task-id>` branch off the default branch.
3. Build the **Claude CLI** `AiProvider` (agentic; `working_dir` = checkout),
   wrap in `RecordingAiProvider` (M13), and run with the full session transcript
   (prior `agent_task_messages`) + the new user `message`. The CLI provider is
   required (it edits files); reject API-only profiles with a clear error.
4. `commit_all` the agent's edits. If nothing changed, append an agent message
   explaining no changes were made and set status `awaiting_input` (don't push an
   empty branch).
5. `push_branch` with the account/M21 token; `open_pull_request` (or update the
   existing PR); store `pr_url` and set status `pr_open`.
6. Append an `agent_task_messages` row (role `agent`) with the agent's summary and
   `execution_id`; record full I/O + tokens on the execution (M13 recorder).
7. Register the type in `AppState::build`; seed a singleton job that backs agent
   tasks (mirroring `llm_request::ensure_job`).

### Routes

Under `require_auth`, alongside the existing `/api/platform/applications/:id/ask*`:

- `GET  /api/platform/applications/:id/agent-tasks` → list tasks (status + PR).
- `POST /api/platform/applications/:id/agent-tasks` → create a task (title +
  first message); enqueues the first turn via `JobRunner::start_with_params`.
- `GET  /api/platform/applications/:id/agent-tasks/:task_id` → task + transcript +
  current execution status + `pr_url`.
- `POST /api/platform/applications/:id/agent-tasks/:task_id/messages` → append a
  follow-up message; enqueues another turn on the same branch.

Resolve the application → repository (+ default branch + token) as the ask handler
does; select the AI profile (configured default or first enabled), requiring a
CLI-capable profile; clear errors for missing profile / non-writable repo / a
provider that can't open PRs.

### UI

A new **"AI Agent"** tab in `assets/platform-app-detail.js` +
`templates/platform_app_detail.html` (added to the `tabs` array next to Use cases
/ Members), using the existing lazy `tabset`:

- A **task list** (title, status badge, PR link when open) + a **"New task"**
  composer (title + first instruction).
- A selected task shows a **chat-style transcript** (user / agent messages) with a
  **follow-up message** box, and a prominent **"View PR"** link once open.
- On submit, POST then poll the task/execution endpoint (reuse the polling pattern
  in `assets/job-detail.js` / the M15 ask poller) until the turn reaches a terminal
  state; show pending, "queued — waiting for the repository" (lock held), and
  failed states clearly. A link to the raw job execution exposes the full agent
  I/O and token usage.

## Tasks

- [ ] `agent_tasks` + `agent_task_messages` migrations (Pg + SQLite) +
      `AgentTaskRepository` (dual-engine) + mock.
- [ ] `GitClient` `create_branch`/`commit_all`/`push_branch` + impls/mocks.
- [ ] `RepositoryProvider` `open_pull_request`/`get_pull_request` (GitHub impl;
      `Unsupported` default) + mocks.
- [ ] `src/agent_tasks/`: `AgentTaskJob` (one turn: lock → branch → agent → commit
      → push → PR), `AgentTaskDeps`, `ensure_job`; register in `AppState::build`.
- [ ] Routes: list/create tasks, post message, poll status.
- [ ] "AI Agent" tab: task list, composer, transcript, follow-up box, PR link,
      polling.
- [ ] Unit tests (mocked git/provider/AI/lock/clock/repo): a turn branches,
      commits, pushes, and opens a PR with correct params; no-change runs don't
      push; a held repo lock yields `CannotRun`; non-CLI profile and PR-unsupported
      provider error clearly; the transcript persists.

## Acceptance criteria

- Creating a task on an application page runs the agent over the repo, opens a PR
  on a dedicated `agent/<task-id>` branch, and shows the PR link.
- A follow-up message refines the change on the same branch and updates the same
  PR; the transcript reflects the full session.
- Two turns for the same repository never run concurrently — the second queues
  behind the per-repo lock.
- A run that produces no edits does not push an empty branch and tells the user.
- Git, provider, AI, lock, and clock are all mocked in unit tests — no network,
  real repo, or real PR.

## Dependencies

Milestones 07 (cloning + repositories/accounts + token), 09 (application detail),
12 (locks), 13 (per-job workspace, per-execution params, reschedule, recorder),
14 (per-repo LLM session pattern over a checkout), 15 (ask UI/polling pattern),
21 (GitHub token/App used to push + open PRs).

## Out of scope

Merging PRs or reacting to PR review comments, running the repo's tests/CI before
opening the PR, providers other than GitHub for PR creation, and an in-browser
diff editor — the agent edits, the PR carries the diff.
