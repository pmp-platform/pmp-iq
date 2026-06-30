# Milestone 36 — Platform diff / timeline & audit

## Goal

Capture **what changed** in the platform model on each sync and **who did what**
in the app, then present a per-application and platform-wide **timeline**, a
**diff** between two points in time, and an **audit log** of mutating actions.
Today re-syncs overwrite the model via delete-and-recreate and nothing records the
delta; only `member → ex_member` transitions are tracked. This generalises that
into first-class change history.

## Scope

- An append-only **change feed** of model mutations emitted by the writer
  (entities, dependencies, ownership, metrics crossing thresholds).
- An **audit log** of operator actions (auth, settings/prompt edits, job runs,
  agent tasks, campaigns).
- Read layer: timeline (per app + global) and `diff(from, to)`.
- UI: a **Timeline / Changes** tab and an admin **Audit** view.

## Deliverables

### Change capture

The `PlatformWriter` (M08) emits a change event whenever an idempotent upsert
results in a real create/update/delete, into an append-only table:

```sql
-- migrate:up
CREATE TABLE platform_changes (
    id             UUID PRIMARY KEY,
    application_id UUID REFERENCES applications(id) ON DELETE CASCADE,
    entity_type    TEXT NOT NULL,   -- application | dependency | library | member | metric | ...
    entity_key     TEXT NOT NULL,   -- natural key, stable across syncs
    change         TEXT NOT NULL,   -- created | updated | removed
    detail         JSONB,           -- before/after summary or threshold crossed
    job_execution_id UUID,          -- the sync that produced it
    occurred_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_platform_changes_app ON platform_changes(application_id, occurred_at DESC);
-- migrate:down
```

Because keys are the existing **natural keys** (the same ones hints/members use to
survive re-sync), the writer can diff the prior set against the new set per sync to
emit precise create/update/remove events instead of churn.

### Audit log

A second append-only table records operator actions (actor, action, target,
metadata, at) written by a small `AuditService` called from the auth middleware
and mutating routes (settings, prompts, jobs, agent tasks, campaigns):

```sql
-- migrate:up
CREATE TABLE audit_events (
    id          UUID PRIMARY KEY,
    actor       TEXT NOT NULL,      -- principal username / "system"
    action      TEXT NOT NULL,      -- login | settings.update | job.run | agent_task.create | ...
    target      TEXT,               -- entity id / route
    metadata    JSONB,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- migrate:down
```

### Read layer & UI

- `timeline(scope, window)` — merged change + audit events for an application or
  the whole platform, newest first, paginated.
- `diff(from, to)` — net created/updated/removed per entity type between two
  timestamps (or two sync executions).
- A **Timeline / Changes** tab on the application detail and a platform-wide
  timeline on the platform section; an **Audit** view (admin-only) over
  `audit_events`. Both reuse the existing list/filter/refresh helpers.

## Tasks

- [ ] `platform_changes` + `audit_events` migrations (both engines) + dual-engine
      repositories.
- [ ] Writer emits create/update/remove change events by diffing prior vs new
      natural-key sets per sync (incl. `member`/`ex_member`, dependencies, metrics
      thresholds).
- [ ] `AuditService` + call sites in auth middleware and mutating routes.
- [ ] `timeline` + `diff` read layer; Timeline tab + admin Audit view.
- [ ] Unit tests (mocked repos): a sync that adds/removes/changes entities emits
      exactly the expected events; `diff` returns the net delta between two points;
      audit events are written for representative actions.

## Acceptance criteria

- Each sync records precise model changes (created/updated/removed) keyed by stable
  natural keys; a timeline and a two-point diff are viewable per app and platform-
  wide.
- Mutating operator actions are recorded in an audit log visible to admins.
- Change emission and diff are unit-tested with mocked storage; both engines
  supported.

## Dependencies

Milestones 08 (writer + natural keys), 03 (auth/principal for the actor), 06/13
(job executions to attribute syncs). Feeds M32/M35 (drift/activity panels) and
M37 (audit gains roles/teams).

## Out of scope

Full point-in-time snapshots/rollback of the model, real-time change streaming,
and external SIEM export — an append-only feed + diff is the scope here.
