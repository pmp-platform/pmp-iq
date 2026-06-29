# Milestone 27 — Configurable job concurrency

## Goal

Today `JobRunner::start_with_params` rejects a new execution with `409 Conflict`
whenever a job already has one running (`count_running(job_id) > 0`). That makes
the singleton AI-Agent job (M22) — and any other job — strictly **one execution
at a time**, so a follow-up turn or a second task fails while one is in flight.

Replace that hard singleton guard with a **configurable per-job max concurrency**
(default 1, preserving today's behaviour) plus an optional **queue** so excess
executions wait instead of failing. Correctness on any single repository is still
guaranteed by the existing per-repository `DistributedLock` (M12) — concurrency
only lets *different* repos/turns proceed in parallel.

## Scope

- A per-job `max_concurrency` setting (with sane defaults; the agent-task job
  defaults higher, configurable via `config.yaml`).
- The runner admits up to `max_concurrency` concurrent executions per job; a
  process-wide cap bounds total parallelism.
- Excess executions **queue** (a `queued` state) and are dispatched as slots free,
  rather than returning `409`.
- The per-repo lock remains the serialisation guarantee for same-repo work.

## Deliverables

### Per-job concurrency setting

- Migration (dual-engine, up-only): add `jobs.max_concurrency INT NOT NULL
  DEFAULT 1`. `JobRepository`/`Job` carry it; the create/update paths accept it.
- `AppState::build` seeds the agent-task job with a configurable default
  (`agent.max_concurrency` in `config.yaml` / `AGENT_MAX_CONCURRENCY`, e.g. 4);
  other built-ins stay at 1.
- A process-wide ceiling (`jobs.max_concurrency` config / `JOBS_MAX_CONCURRENCY`)
  caps total concurrent executions across all jobs to protect resources.

### Runner: admit, don't reject

`JobRunner` (`src/jobs/runner.rs`):

- `start_with_params` admits when `count_running(job_id) < job.max_concurrency`
  **and** the global cap isn't exceeded; on admission it spawns `run_execution`
  as today.
- When at capacity, instead of `409` it creates the execution in a new **`queued`**
  `ExecStatus` (the request still returns an `execution_id` the UI can poll), and
  does **not** spawn it yet.
- Keep an explicit "is this exact execution already running" guard so a retry of
  the *same* execution can't double-run.

### Queue dispatch

- The leader-elected `JobController` loop (M13) gains a **dispatcher** step: for
  each job, while `count_running < max_concurrency` and the global cap allows,
  start the oldest `queued` execution (`resume`-style spawn).
- A queued agent turn whose repo is busy still reschedules via the per-repo lock
  (`CannotRun`, M13) — concurrency and the lock compose: the slot opens, the turn
  starts, finds the repo locked, and reschedules without consuming the slot
  uselessly (release the slot on `CannotRun`).
- `JobExecutionRepository` gains `count_running(job_id)`, `list_queued`, and a
  claim that flips `queued → running` atomically (so two instances don't start the
  same queued execution).

### UI/observability

- Executions show a `queued` state (badge) and their position is implicit by
  `created_at`; the agent-task UI shows "queued — waiting for a slot" distinct
  from "queued — waiting for the repository" (lock).
- The job detail surfaces the job's `max_concurrency`.

## Tasks

- [ ] `jobs.max_concurrency` migration (Pg + SQLite) + `Job`/repository plumbing.
- [ ] `ExecStatus::Queued`; `count_running`/`list_queued`/atomic claim on the
      executions repository.
- [ ] Runner admits up to `max_concurrency` (+ global cap); queues the rest with
      an `execution_id`; no more blanket `409`.
- [ ] Controller dispatcher starts queued executions as slots free; releases the
      slot on `CannotRun`.
- [ ] Config (`agent.max_concurrency`, `jobs.max_concurrency`) wired in M18.
- [ ] UI: queued state + reason (slot vs repo lock); job shows its concurrency.
- [ ] Unit tests (mocked repos/clock/lock): N executions run concurrently up to
      the limit; the N+1 queues (not 409); the dispatcher starts it when a slot
      frees; the global cap is honoured; the atomic claim prevents double-start.

## Acceptance criteria

- A job's concurrency is configurable; with the default (1) behaviour is
  unchanged.
- The agent-task job runs multiple turns/tasks in parallel up to its limit;
  same-repo turns still serialise via the per-repo lock; nothing double-runs.
- Excess executions queue and start automatically as slots free, instead of
  failing with `409`.
- Admission, queueing, dispatch, and the global cap are unit-tested with mocked
  dependencies; no test touches a real clock or database.

## Dependencies

Milestones 06/13 (jobs runner, controller loop, executions repository), 12
(per-repo locks), 18 (config), 22 (the singleton agent-task job this unblocks).

## Out of scope

Priority queues / fair scheduling across jobs (FIFO by `created_at`), autoscaling
workers, and cross-instance work-stealing beyond the leader-dispatched queue.
