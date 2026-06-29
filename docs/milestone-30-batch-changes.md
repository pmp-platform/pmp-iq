# Milestone 30 — Batch changes: large-scale edits across many repos

## Goal

A **campaign** capability for applying the same change across **many**
repositories at scale (tens–hundreds) — the Sourcegraph Batch Changes pattern.
Where a multi-repo agent task (M23) is one conversational session spanning a few
repos, a campaign is a **declarative, repeatable** spec executed over a large,
filtered repository set with **dry-run preview**, **progress tracking**, and
**bulk PR management**. The M24/M25 PR watcher then keeps every campaign PR
healthy automatically.

## Scope

- A named campaign = target selection + a transformation + a PR template.
- Transformations: a deterministic codemod/script step, an LLM instruction, or
  both (script first, LLM to finish/adapt).
- Dry-run preview (diffs without pushing) before mass-opening PRs.
- Fan-out execution bounded by configurable concurrency (M27), one branch+PR per
  repo, serialised per repo by the lock.
- A campaign dashboard: per-repo status, aggregate progress, and bulk actions.

## Deliverables

### Campaign spec & persistence

```sql
-- migrate:up
CREATE TABLE campaigns (
    id           UUID PRIMARY KEY,
    name         TEXT NOT NULL,
    selection    JSONB NOT NULL,    -- explicit repo ids or a PlatformQuery filter
    transform    JSONB NOT NULL,    -- { script?: string, instruction?: string }
    pr_title     TEXT NOT NULL,
    pr_body      TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'draft', -- draft|previewing|running|done|failed
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE campaign_repos (
    id            UUID PRIMARY KEY,
    campaign_id   UUID NOT NULL REFERENCES campaigns(id) ON DELETE CASCADE,
    repository_id UUID NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending', -- pending|changed|no_change|pr_open|merged|failed
    pr_url        TEXT,
    diff_summary  TEXT,             -- dry-run / applied summary
    UNIQUE (campaign_id, repository_id)
);
-- migrate:down
```

The transformation reuses the agent-task machinery (M22 turn: branch → apply →
commit → push → PR); a campaign owns one task/target per repo (it may reuse
`agent_task_targets` keyed by `campaign_id`, or the parallel structure above).

### Target selection

Resolve to a concrete repository set from either an explicit list or a
`PlatformQuery` filter (M09) — e.g. "every application that depends on library X"
or "all `ecosystem = npm` apps". The same `resolve_targets` helper as M23.

### Execution & concurrency

- **Dry-run**: run the transform on each repo's checkout and record `diff_summary`
  + `status` (`changed`/`no_change`) **without pushing**, so the operator reviews
  the blast radius first.
- **Apply**: fan out one turn per repo, **bounded by M27 concurrency** and the
  per-repo lock; each repo branches, applies the transform (script and/or LLM),
  commits, pushes, and opens a PR from the template. No-change repos are skipped.
- A deterministic `script` step runs via the `CommandRunner` in the checkout
  (sandboxed), so simple mechanical changes don't need the LLM; the `instruction`
  lets the agent handle the parts a script can't.

### Dashboard & bulk actions

- A **campaign view**: per-repo status table (pending/changed/no-change/pr-open/
  merged/failed), aggregate rollups (e.g. "37/120 merged"), and a progress bar
  that tracks **merge progress over time** (migration tracking).
- **Bulk actions**: retry failed, re-run dry-run, open all reviewed PRs, close all
  open PRs, and re-target. Per-repo drill-down opens that repo's task transcript
  and PR.
- The **PR watcher** (M24) automatically fixes comments/conflicts/failed checks
  across the campaign's PRs and finishes merged/closed ones — no per-repo babysitting.

## Tasks

- [ ] `campaigns` / `campaign_repos` migrations + repository.
- [ ] Selection (explicit or `PlatformQuery` filter) → repository set.
- [ ] Transform execution: deterministic `script` (CommandRunner) + LLM
      `instruction`, reusing the agent turn (branch/commit/push/PR).
- [ ] Dry-run (diff + status, no push) vs apply (fan-out, M27-bounded, per-repo
      lock).
- [ ] Campaign dashboard: per-repo status, rollups, merge-progress, bulk actions.
- [ ] Unit tests (mocked git/provider/runner/lock/repo): dry-run records diffs
      without pushing; apply opens one PR per changed repo; no-change repos
      skipped; concurrency cap honoured; bulk retry re-runs only failed repos.

## Acceptance criteria

- An operator defines a campaign once and applies it across a filtered set of many
  repositories, previewing diffs before opening PRs.
- Execution fans out concurrently (bounded by M27) with one PR per repo, serialised
  per repo; the dashboard tracks per-repo and aggregate progress (incl. merges).
- Bulk actions (retry/close/re-run) work; the PR watcher keeps campaign PRs
  healthy.
- Selection, dry-run, apply, and bulk actions are unit-tested with mocked deps.

## Dependencies

Milestones 22/23 (agent turns + multi-repo), 27 (configurable concurrency for the
fan-out), 24/25 (PR watcher keeps PRs healthy), 09 (selection filters),
`src/process.rs` (`CommandRunner` for the deterministic script step).

## Out of scope

A codemod DSL/registry of prebuilt transforms (the `script`/`instruction` is
free-form), cross-repo transactional merges, and scheduling campaigns (run
on-demand; recurring campaigns can layer on the scheduler later).
