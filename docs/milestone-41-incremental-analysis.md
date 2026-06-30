# Milestone 41 — Incremental analysis

## Goal

Stop re-analyzing whole repositories on every sync. On a push (webhooks, M25,
already enqueue a scoped re-sync) or a scheduled run, detect the files changed
since the **last analyzed commit** and re-run analysis only for the **affected
entities** (the components/use cases attributed to those files), merging the
result into the existing model instead of the current delete-and-recreate. This
cuts LLM cost and latency dramatically for large fleets where most syncs touch a
handful of files.

## Scope

- Persist the last-analyzed commit SHA per repository; diff against the new HEAD.
- Map changed files → affected entities using the existing file attribution (M17).
- An incremental analyzer mode and a **partial** writer upsert (don't delete
  untouched entities), with a safe **full-reanalysis fallback**.
- Wire webhooks/scheduled syncs to incremental by default; keep a force-full path.

## Deliverables

### Change detection

- `RepoRecord` gains `last_analyzed_sha`; after a successful analysis the sync
  records the analyzed HEAD.
- `GitClient::changed_files(from_sha, to_sha)` → the set of changed/added/removed
  paths (git2 diff). When `from_sha` is absent (first analysis) or unreachable
  (force-push/rebase), fall back to full analysis.

### Affected-entity mapping

The analyzer (M17) already records which repository files each **use case** and
**component** affects. Inverting that attribution maps the changed file set to the
affected components/use cases. **Structural triggers force a full re-analysis**:
changed manifests/lockfiles, CI config, or a changed default branch — anything
that can alter applications, dependencies, libraries, or members (which aren't
file-attributed).

### Incremental analyze & partial write

- An `analyze_incremental(input, changed, affected)` path re-extracts only the
  affected sections and returns a **partial** `AnalysisResult`.
- `PlatformWriter` gains a **partial** mode: upsert the affected components/use
  cases (and their use_case_components/diagrams/observability) without the
  delete-and-recreate of untouched siblings; `prune_orphans` still runs at the end
  of a full sweep only.
- Hints, members, and shared entities continue to reconcile as today on full
  syncs; incremental syncs touch only the affected app's sub-entities.

### Job wiring

- The `sync-repositories` job accepts `incremental: bool` (default true for
  webhook/scheduled runs scoped to a repository; full-fleet manual runs stay full,
  or expose a "force full" toggle).
- The execution log states the mode, the diff size, and the affected entity count;
  a full-fallback reason is logged when triggered.

## Tasks

- [ ] `last_analyzed_sha` on `RepoRecord` (migration, both engines) + record on
      success.
- [ ] `GitClient::changed_files(from,to)` (git2 diff) + mock; full-fallback when
      base missing/unreachable.
- [ ] Invert M17 file attribution → affected components/use cases; structural-file
      triggers force full.
- [ ] `analyze_incremental` + `PlatformWriter` partial upsert (no delete of
      untouched entities; prune only on full sweep).
- [ ] `incremental` job param; webhook/scheduled scoped runs default to it; force-
      full path; informative logs.
- [ ] Unit tests (mocked git/analyzer/writer): a small diff re-analyzes only the
      attributed entities and leaves others intact; a manifest change forces full;
      a missing base SHA falls back to full; `last_analyzed_sha` advances.

## Acceptance criteria

- A sync over a small change re-analyzes only the affected components/use cases and
  merges them, leaving untouched entities (and their attribution) intact.
- Structural changes (manifests/CI/default branch) and missing/unreachable base
  commits safely fall back to full analysis.
- Webhook/scheduled scoped syncs run incrementally by default with a force-full
  option; behaviour is unit-tested with mocked git/analyzer/writer.

## Dependencies

Milestones 08 (analyzer + writer), 17 (file attribution per use case/component),
25 (webhooks scoped re-sync), 07/13 (repositories + per-job workspace + recorder).
Reduces cost tracked by M39; complements M40 (`summary_hash` re-embed).

## Out of scope

Per-file streaming analysis, cross-repository incremental dependency
recomputation, and incremental membership/library reconciliation — those remain
on the full sweep; this targets file-attributed app sub-entities.
