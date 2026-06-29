# Milestone 14 — `llm-repository-request` job type

## Goal

Add a new pluggable job type, **`llm-repository-request`**, that runs an LLM
session against a single repository. Given an input prompt and a repository
(plus an optional branch, default `main`), it ensures the repo is cloned and up
to date (clone if missing, then **fetch && rebase** the branch), runs the LLM
with the input over the checkout, and returns the model's output. It serialises
per repository with a **distributed lock keyed by the repository name**, and —
via the M13 recorder — captures the full LLM input/output and token metadata.

## Scope

- A `llm-repository-request` `JobType` taking per-execution input.
- Clone-if-missing + fetch-and-rebase of the selected branch into the per-job
  workspace.
- Running the configured `AiProvider` against the checkout and returning its
  output.
- A per-repository distributed lock; concurrent requests for the same repo are
  rescheduled (M13 `CannotRun`), not run in parallel.
- Recording the LLM I/O to the execution output and usage to `metadata`.

## Deliverables

### Job input (per-execution `params`)

Carried on the execution `params` (M13), so the same configured job serves many
ad-hoc questions:

```json
{ "repository": "<full_name or repository id>",
  "branch": "main",
  "input": "<the user prompt>",
  "ai_profile_id": "<uuid>" }
```

`branch` defaults to `main`. The job resolves `repository` against
`RepoRecordRepository` / the accounts to obtain the clone URL, default branch,
and any clone token.

### Git: fetch + rebase a branch

The existing `GitClient::clone_or_update` fetches but does not move the working
tree to the branch head. Extend the git abstraction so the checkout reflects the
latest remote branch:

- Add `sync_branch(request)` (or extend `run_clone`) that, for an existing
  checkout, fetches `origin` then fast-forwards / rebases the working tree onto
  `origin/<branch>`; for a missing checkout it performs the fresh clone on that
  branch. Reuse `CloneRequest` (`clone_url`, `dest`, `branch`, `token`).
- Keep all `git2` work on a blocking thread (as today).

### LLM session over the checkout

The session must see the repository:

- Add `cwd: Option<String>` to `CommandSpec` (`src/process.rs`) and honour it in
  `TokioCommandRunner` (sets the child process working directory).
- Add `working_dir: Option<String>` to `AiRequest`. The **Claude CLI** provider
  runs in that directory (agentic — it can read the repo files); the **Anthropic
  API** provider, which has no filesystem access, gathers a bounded file-context
  bundle from the checkout (reuse the analyzer's context-gathering helper) and
  prepends it to the prompt. Document that full agentic repo inspection requires
  the CLI provider.

### The job

A new `src/llm_request/` module (mirroring `src/review/`):

```rust
pub const JOB_TYPE: &str = "llm-repository-request";
```

`run(ctx)`:
1. Parse `ctx.params` into a typed `LlmRequestInput`; resolve the repository and
   branch.
2. Acquire `DistributedLock` on `lock_keys::repository(full_name)` with a TTL; if
   not granted, return `JobError::CannotRun { retry_at }` (M13 reschedules it).
   `refresh` the lease during the run; `release` at the end (including on error).
3. Clone-if-missing + `sync_branch` into `Workspace::repo_dir(job_dir, full_name)`
   (M13 per-job workspace; the checkout persists across runs).
4. Build the `AiProvider` from `ai_profile_id`, wrap it in `RecordingAiProvider`
   (M13), and `complete(AiRequest::new(input).with_working_dir(checkout))`.
5. Return `JobOutcome::Completed { summary }`. The full prompt+response is already
   in the execution output and token usage in `metadata`; also store the answer
   text in `metadata.answer` (and/or `summary`) so callers (M15) can render it
   without re-parsing the output stream.
6. Register the type in `AppState::build`; seed a single built-in
   `llm-repository-request` job so executions can be enqueued against it.

Dependencies are bundled in an `LlmRequestDeps` struct (accounts/repositories,
git, workspace, AI service + deps, lock, clock) to stay within the parameter
limit.

## Tasks

- [ ] `GitClient::sync_branch` (fetch + rebase/fast-forward a branch) + impl/mock.
- [ ] `CommandSpec.cwd` honoured by `TokioCommandRunner`; `AiRequest.working_dir`
      used by the CLI provider; API provider gathers checkout context.
- [ ] `src/llm_request/` module: `LlmRequestInput`, `LlmRequestDeps`,
      `LlmRepositoryRequestJob` implementing `JobType`.
- [ ] Per-repository lock acquire/refresh/release with `CannotRun` reschedule.
- [ ] Record LLM I/O + tokens; store the answer in `metadata`.
- [ ] Register + seed the built-in job in `AppState::build`.
- [ ] Unit tests (mocked git/lock/provider/clock/fs): clone-missing vs
      fetch-rebase paths; a held repo lock yields `CannotRun`; the answer and
      tokens are recorded; failures release the lock.

## Acceptance criteria

- Triggering `llm-repository-request` with an input over a real repository clones
  (or updates) it, runs the LLM against the checkout, and returns the answer.
- Two requests for the same repository do not run concurrently — the second is
  rescheduled until the first releases the lock.
- The execution output contains the full LLM input and output; `metadata` holds
  the answer and token usage.
- Git, lock, provider, and clock are all mocked in unit tests — no network, real
  repo, or real clock.

## Dependencies

Milestones 05 (AI providers), 07 (cloning + repositories), 12 (locks), 13 (jobs:
workspaces, params, reschedule, recorder).

## Out of scope

The application-detail "ask the LLM" UI (M15) and entity hints / file features
(M16–M17).
