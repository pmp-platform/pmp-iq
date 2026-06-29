# Milestone 13 — Job scheduling, per-job workspaces & live updates

## Goal

Evolve the jobs subsystem so that:

1. every job runs inside its **own workspace directory**,
2. jobs are time-scheduled via a **`next_run_at`** field polled under a **leader
   lock**,
3. a job can **decline to run** (returning a typed reschedule signal instead of
   failing) when it can't take its resource lock,
4. jobs can **stream raw output and structured metadata** while running, and
5. any job that calls an LLM **records the full LLM input + output** to its output
   and **persists usage metadata** (tokens and anything the model returns) on
   completion.

The existing `sync-repositories` job is brought onto all of the above and made to
take a distributed lock.

## Scope

- Per-job workspace directories `{WORKSPACE_DIR}/jobs/{job-name}/{job-id}`.
- `jobs.next_run_at` + a leader-gated scheduler poll.
- A typed "cannot run now, retry at T" outcome the runner reschedules instead of
  marking failed.
- Per-execution `metadata` (live-updatable JSONB) and per-execution input
  `params`; the existing `logs` column reused as the live raw **output**.
- A reusable LLM-run recorder shared by every job that uses an `AiProvider`.
- Wire `sync-repositories` onto the lock, the recorder, and the per-job workspace.

## Deliverables

### Per-job workspace

Extend `Workspace` (`src/workspace.rs`) so each job gets a stable directory that
**persists across executions** (enabling clone reuse / fetch-and-update):

- `job_dir(job_name, job_id) -> "{root}/jobs/{sanitize(job_name)}/{job_id}"`.
- `repo_dir` nests a repository under that job dir (reuse the existing
  `sanitize`). Pass a small `WorkspaceTarget { job_name, job_id, full_name }`
  struct to stay within the parameter limit.
- Update `review/job.rs::do_clone` to use the job dir (it currently keys the
  path by `execution_id`, so each run re-clones; keying by `job_id` lets
  `git clone_or_update` fetch into the existing checkout).

### Scheduling via `next_run_at`

Migration (`jobs` table):

```sql
-- migrate:up
ALTER TABLE jobs ADD COLUMN next_run_at TIMESTAMPTZ;
CREATE INDEX idx_jobs_next_run ON jobs(enabled, next_run_at);
-- migrate:down
```

- `Job` / `JobInput` gain `next_run_at: Option<DateTime<Utc>>`.
- `JobRepository`: `list_due(now)` (enabled jobs with `next_run_at <= now`) and
  `set_next_run_at(id, when)`.
- A leader-gated poll (extend `JobController` / the scheduler): each tick, if it
  holds the controller lock (M12 `DistributedLock`), it lists due jobs and
  `start`s each via the `JobRunner`.
- After a run **completes**, recompute `next_run_at`: from `cron_expr` (next fire)
  when set, otherwise `NULL` (one-shot scheduled run). Time comes from `Clock`.

### Decline-to-run → reschedule (not failure)

A job that can't run right now (e.g., it couldn't take its resource lock)
returns a typed signal carrying the next time to try:

```rust
pub enum JobError {
    Failed(String),
    /// The job could not run now; reschedule rather than fail.
    CannotRun { retry_at: Option<DateTime<Utc>> },
}
```

Runner behaviour (`run_execution`) on `CannotRun`:
- Does **not** mark the execution `failed`. Records a terminal, non-error status
  (`ExecStatus::Skipped`, added alongside the existing variants) with a short note.
- Sets `jobs.next_run_at = retry_at.unwrap_or(now + 5 minutes)` so the job stays
  pending and is retried on a later poll.

### Per-execution input + live output & metadata

Migration (`job_executions` table):

```sql
-- migrate:up
ALTER TABLE job_executions ADD COLUMN params   JSONB NOT NULL DEFAULT '{}';
ALTER TABLE job_executions ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}';
-- migrate:down
```

- **Output**: reuse the existing `logs` TEXT column as the live raw output stream
  (already appended via `LogSink` and shown live by the UI). No new column.
- **Metadata**: new `metadata` JSONB the job may update mid-run or set on
  completion. `summary` keeps its current meaning (terminal stats from
  `JobOutcome::Completed`); `metadata` is the new free-form, incrementally
  updatable store.
- **Params**: per-execution input. `JobRunner::start(job_id, trigger, params)`
  writes it onto the execution; `JobContext` exposes `params` (used by the
  ad-hoc `llm-repository-request` runs in M14/M15).
- `JobExecutionRepository`: `merge_metadata(id, patch)` (shallow object merge) and
  `append_output(id, text)` (or keep using `LogSink::append`).
- `JobContext` helpers: `append_output(&str)`, `merge_metadata(&Value)`, plus the
  existing `log`, `save_state`, `pause_requested`.

### Reusable LLM-run recorder

So that **every** LLM-using job satisfies "show all the input + output of the LLM,
save token metadata when done", add a decorator implementing `AiProvider`:

- `RecordingAiProvider { inner: Arc<dyn AiProvider>, sink: ExecutionSink }`.
- On `complete(request)`: appends a formatted block to the execution output
  (the system prompt + prompt, then the response text), merges accumulated
  `{ input_tokens, output_tokens }` into the execution `metadata`, and returns the
  inner `AiResponse` unchanged.
- Jobs wrap their provider once (`RecordingAiProvider::new(provider, ctx)`),
  keeping the recording logic in one reused place rather than per job.

### Wire `sync-repositories`

- Acquire a `DistributedLock` (key `lock_keys::job(job_id)`) at the start of
  `run`; if not granted, return `JobError::CannotRun { retry_at: now + 5m }`.
- `refresh` the lease between accounts during the long sweep; `release` on
  completion/pause.
- Wrap its `AiProvider` with `RecordingAiProvider` so analysis prompts/responses
  and token totals land in the execution output/metadata.

## Tasks

- [ ] `Workspace::job_dir` + per-job pathing; move the sync job onto it.
- [ ] `jobs.next_run_at` migration; `Job`/`JobInput`; `list_due`/`set_next_run_at`.
- [ ] Leader-gated due-job poll in the controller; post-run `next_run_at` recompute.
- [ ] `JobError::CannotRun` + `ExecStatus::Skipped`; runner reschedule path.
- [ ] `params` + `metadata` migration; repo `merge_metadata`/`append_output`;
      `JobContext` helpers; `JobRunner::start(..., params)`.
- [ ] `RecordingAiProvider` decorator.
- [ ] Lock + recorder + job dir wired into `sync-repositories`.
- [ ] Unit tests (mocked repos/clock/lock): due-job selection; `CannotRun`
      reschedules (not fails) with the given/default time; metadata merge;
      recorder writes I/O + tokens; scheduler runs only as leader.

## Acceptance criteria

- A scheduled job with `next_run_at` is picked up by the leader and run; cron
  jobs re-arm their next fire; one-shots clear.
- A job that can't take its lock is rescheduled (job stays pending, execution
  `skipped`) — never `failed`.
- Each job runs under `{WORKSPACE_DIR}/jobs/{name}/{id}`.
- Output and metadata update live; an LLM-using run shows full prompt+response in
  its output and token usage in `metadata`.
- All logic is unit-tested with mocked dependencies (no real DB, clock, timers,
  or network).

## Dependencies

Milestones 06 (jobs), 08 (sync job analysis), 12 (distributed locks).

## Out of scope

The `llm-repository-request` job type (M14) and the application-facing UIs
(M15–M17).
