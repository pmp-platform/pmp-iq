# Milestone 06 — Jobs infrastructure & scheduler

## Goal

Build the generic **Jobs** subsystem: typed job definitions, multiple trigger
kinds (manual, cron), an execution runner with status tracking, and a Jobs
section that lists executions. This milestone ships the framework; the first
concrete job type is implemented in M07–M08.

## Scope

- `jobs` and `job_executions` tables + data-access traits.
- `JobType` trait so job types are pluggable; a registry of types.
- Trigger handling: manual run + cron scheduler.
- Async execution with status, timing, logs, and a summary per run.
- Jobs UI: list/configure jobs, list executions with status, view a run's detail.

## Deliverables

### Data model

```sql
-- migrate:up
CREATE TABLE jobs (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_type      TEXT NOT NULL,            -- e.g. 'review-repositories'
    name          TEXT NOT NULL,
    trigger_type  TEXT NOT NULL,            -- 'manual' | 'cron'
    cron_expr     TEXT,                     -- when trigger_type = 'cron'
    config        JSONB NOT NULL DEFAULT '{}',
    enabled       BOOLEAN NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE job_executions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id       UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    status       TEXT NOT NULL,             -- 'queued'|'running'|'succeeded'|'failed'|'cancelled'
    trigger      TEXT NOT NULL,             -- 'manual' | 'cron'
    started_at   TIMESTAMPTZ,
    finished_at  TIMESTAMPTZ,
    summary      JSONB,                     -- per-run stats (counts, durations)
    error        TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_job_executions_job ON job_executions(job_id, created_at DESC);

-- migrate:down
```

### Job abstraction

- `JobType` trait:
  - `id(&self) -> &str` (stable type key).
  - `run(&self, ctx: JobContext) -> Result<JobOutcome, JobError>` where
    `JobContext` bundles the execution id, parsed config, repositories/services,
    and a progress/log sink (a struct — keeps params ≤ 4). `JobOutcome` carries
    the summary stats.
- A `JobTypeRegistry` maps `job_type` strings to implementations.
- A `JobRunner` service: creates a `job_executions` row, transitions status,
  captures logs and errors, records timing and summary. Time comes from an
  injected `Clock` trait for deterministic tests.

### Triggers & scheduling

- Manual: `POST /api/jobs/{id}/run` enqueues an execution.
- Cron: a scheduler (e.g. `tokio-cron-scheduler`) reads enabled cron jobs and
  enqueues them; behind a `Scheduler` trait so it can be faked in tests.
- Concurrency control so the same job doesn't overlap itself.

### Execution logging

- Per-execution log lines streamed to storage (table or file) so the UI can show
  progress; abstracted behind a `LogSink` trait.

### UI

- Jobs → list of configured jobs (type, trigger, enabled) with create/edit and a
  "Run now" button.
- Executions table: job, status badge, started/finished, duration, trigger.
- Execution detail: summary, logs, error. jQuery polls for live status.

## Tasks

- [ ] Migrations for `jobs` and `job_executions`.
- [ ] `JobRepository` / `JobExecutionRepository` traits + sqlx impls + mocks.
- [ ] `JobType` trait, `JobContext`, `JobOutcome`, `JobTypeRegistry`.
- [ ] `JobRunner` with status lifecycle, `Clock`, and `LogSink` traits.
- [ ] `Scheduler` trait + cron impl; manual-run endpoint.
- [ ] Jobs UI: job CRUD, executions table, execution detail with polling.
- [ ] Unit tests: runner lifecycle (success/failure/cancel) with a stub job,
      mocked clock and repositories; scheduler trigger logic.

## Acceptance criteria

- A no-op test job can be created, run manually, and scheduled via cron; its
  execution appears with correct status, timing, and logs.
- Failures are captured (status `failed` + error) without crashing the runner.
- Runner and scheduler logic are unit-tested with mocks — no real DB, clock, or
  timers.

## Dependencies

Milestones 01–03 (DB, settings shell, auth).

## Out of scope

The `review-repositories` job's behaviour (M07 cloning, M08 analysis).
