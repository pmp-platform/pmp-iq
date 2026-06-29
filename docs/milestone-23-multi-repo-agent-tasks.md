# Milestone 23 — Multi-repository AI Agent tasks

## Goal

Extend the AI Agent tasks (M22) so a single task can span **multiple
repositories** — for cross-cutting changes such as "rename this API endpoint in
the service and update every caller". Today a task is bound to one application /
repository and opens one PR. A multi-repo task targets a **set** of repositories,
opens (and tracks) **one PR per repository**, runs each repo's work behind its
own per-repository lock, and shares the overall instruction + cross-repo context
across them.

## Scope

- A task that targets N repositories instead of exactly one, each with its own
  branch, PR, and status.
- Selecting the target set explicitly **or** from a platform-model filter (e.g.
  "all applications that depend on library X").
- Per-repo turns reusing the M22 machinery (branch → edit → commit → push → PR),
  serialised per repository, with the shared task instruction as context.
- A task view that shows per-repository PR status and a combined transcript.

## Deliverables

### Persistence

Generalise a task to many targets (dual-engine migration, up-only). The parent
`agent_tasks` keeps the title + overall status; each repository becomes a target
row carrying its own branch/PR/status:

```sql
-- migrate:up
CREATE TABLE agent_task_targets (
    id            UUID PRIMARY KEY,
    task_id       UUID NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    repository_id UUID NOT NULL,
    branch_name   TEXT NOT NULL,        -- agent/<task-id> (stable per target)
    status        TEXT NOT NULL DEFAULT 'pending',  -- pending|running|pr_open|awaiting_input|failed
    pr_url        TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (task_id, repository_id)
);
-- migrate:down
```

The existing single-repo task is the one-target case (a migration backfills a
target row from `agent_tasks.repository_id`; the column can stay for the primary
target or be dropped once reads move to `agent_task_targets`).
`AgentTaskRepository` gains target CRUD + `update_target_status`.

### Target selection

Two ways to choose the repositories, both resolved to a concrete `repository_id`
set at task-creation time:

- **Explicit** — a list of application/repository ids from the UI.
- **From the platform model** — a `PlatformQuery` filter (reuse `ListQuery` /
  `filter_fields`), e.g. applications by `library`, `service`, `kind`, or
  (when M-ownership lands) team. The matched applications resolve to their
  `repository_id` via `application_repository`.

A `resolve_targets(selection)` helper returns the repository set; the route
records one `agent_task_targets` row per repo.

### The job (per-target turns)

`AgentTaskJob` (M22) is extended to a turn over a **single target**:

- Per-execution `params` carry `{ task_id, target_id, message, ai_profile_id }`.
- The turn acquires the **per-repository** lock for that target (M12), so
  different repos progress independently and the same repo never runs twice at
  once; contention reschedules (`CannotRun`, M13).
- The agent prompt includes the shared task instruction **plus** a short summary
  of the other targets ("this change also touches services A and B") so edits
  stay consistent across repos.
- Commit → push → open/update PR exactly as M22; the result updates the
  **target** status/`pr_url`, and the parent task's status is derived (e.g.
  `pr_open` once any target has a PR; `completed` when all targets are
  merged/closed — see M24).

Creating a task fans out: enqueue one turn per target. Because the singleton job
serialises executions, targets process one at a time by default; document that
per-repo locks already make parallel execution safe if the job-level concurrency
guard is later relaxed.

### Routes & UI

- `POST /api/platform/agent-tasks` (app-agnostic) accepts a target **selection**
  (explicit ids or a filter) + title + instruction; the per-application route
  (M22) remains the single-target shortcut.
- The task detail returns the parent task + its targets (each with repo name,
  status badge, PR link) + the transcript.
- UI: the "New task" composer gains a multi-select / "from filter" mode; the task
  view lists each repository's PR and status; a follow-up message re-runs the
  affected targets.

## Tasks

- [ ] `agent_task_targets` migration (Pg + SQLite) + backfill; repository target
      CRUD + status updates.
- [ ] `resolve_targets` (explicit ids or `PlatformQuery` filter) → repository set.
- [ ] Per-target turn in `AgentTaskJob` (`target_id` param; per-repo lock);
      derive parent status from targets.
- [ ] App-agnostic create route + fan-out one turn per target; task detail
      returns targets + PRs.
- [ ] UI: multi-repo composer + per-repository PR/status view.
- [ ] Unit tests (mocked git/provider/lock/repo): fan-out enqueues one turn per
      target; a held repo lock reschedules only that target; parent status
      derives from target statuses; filter-based selection resolves repositories.

## Acceptance criteria

- A single task can target several repositories, opening and tracking one PR per
  repo, with a combined transcript and per-repo status.
- Targets can be chosen explicitly or via a platform-model filter.
- Each repository's work is serialised by its own lock; one repo being busy
  reschedules only that target, not the whole task.
- Selection, fan-out, and status derivation are unit-tested with mocked deps.

## Dependencies

Milestones 22 (AI Agent tasks), 12 (per-repo locks), 13 (per-execution params /
reschedule), 09 (`PlatformQuery` filters / `application_repository`).

## Out of scope

Cross-repo atomicity (each PR is independent; there is no two-phase merge),
automatic dependency-order sequencing of the PRs, and conflict/comment handling
on the resulting PRs (that is M24).
