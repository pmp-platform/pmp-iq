# Milestone 43 — Production-readiness scorecards

## Goal

Turn the signals already in the platform model — quality metrics (M31/M33),
ownership (M37), CI/observability/diagram presence (derived metrics),
dependencies, members and the change feed — into a per-application
**production-readiness scorecard**: a set of pass/fail/weighted **checks** rolled
into a score and a maturity **level** (e.g. bronze/silver/gold), browsable per
application and aggregated by team. This is the synthesis layer that makes pmp-iq
a governance tool, not just a viewer: "is this service ready, and what's missing?"

## Scope

- A configurable set of **checks** (id, description, weight, severity, the
  signal it evaluates) with shipped defaults.
- A pure **scoring engine** that evaluates checks against an application's
  model + latest metrics and computes a score + level.
- A scorecard job/step that records results with history (for trends, M35).
- Per-app, fleet and per-team scorecard views; check results gate nothing by
  themselves but feed M46 auto-remediation.

## Deliverables

### Checks & results

```sql
-- migrate:up
-- Check definitions (seeded from code defaults; editable, like extraction
-- prompts M34). `rule` names a built-in evaluator + its parameters.
CREATE TABLE scorecard_checks (
    id          TEXT PRIMARY KEY,          -- has_owner | coverage_min | has_ci | no_critical_vulns | ...
    description TEXT NOT NULL,
    rule        TEXT NOT NULL,             -- built-in evaluator key
    params      JSONB NOT NULL DEFAULT '{}',
    weight      INT  NOT NULL DEFAULT 1,
    severity    TEXT NOT NULL DEFAULT 'warn', -- info | warn | critical
    enabled     BOOLEAN NOT NULL DEFAULT TRUE
);
-- Per-application, per-check results (history kept for trends).
CREATE TABLE scorecard_results (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    check_id       TEXT NOT NULL,
    passed         BOOLEAN NOT NULL,
    detail         JSONB,                  -- the observed value vs threshold
    evaluated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_scorecard_results_app ON scorecard_results(application_id, evaluated_at DESC);
-- migrate:down
```

### Scoring engine

A pure `evaluate(app_detail, metrics, ownership, checks) -> Scorecard` that runs
each enabled check's built-in evaluator. Built-in rules read existing signals:

- `has_owner` — the app has an owning team (M37) or codeowner grant (M08).
- `coverage_min` / `complexity_max` / `duplication_max` — latest metric vs a
  `params` threshold (M31/M33).
- `has_ci` / `has_tests` / `has_observability` / `has_diagrams` — derived metrics.
- `no_critical_vulns` — from the security metrics (M31) / SBOM (future).
- `documented` — application/component descriptions are non-empty.

The score is the weighted fraction of passed checks; the **level** comes from
configurable thresholds (e.g. < 0.5 bronze, < 0.85 silver, else gold), with any
failed `critical` check capping the level. The engine is unit-tested on fixed
inputs (no I/O), like the M32 dashboard `build`.

### Wiring & UI

- Scoring runs as a step after metrics collection (M31) and on demand, writing
  `scorecard_results`; results feed M35 trends (`score` as a metric).
- `GET /api/platform/applications/:id/scorecard` (checks + score + level),
  `GET /api/platform/scorecards` (fleet, sortable), and a team rollup that keys
  on M37 teams.
- A **Scorecard** tab on the application detail (each check with pass/fail and the
  observed value), a fleet scorecard table, and a "by team" breakdown on the
  Insights dashboard (M32) with the M35 treemap coloured by level.

## Tasks

- [ ] `scorecard_checks` + `scorecard_results` migrations (both engines) +
      dual-engine repository; seed default checks.
- [ ] Pure `evaluate()` scoring engine over model + metrics + ownership; level
      thresholds with critical-check capping.
- [ ] Scoring step after metrics collection + on-demand route; record history.
- [ ] App scorecard tab, fleet scorecard, per-team rollup; treemap by level.
- [ ] Unit tests (fixed dataset): each rule passes/fails correctly; weighted score
      + level computed; a failed critical check caps the level; empty-signal safe.

## Acceptance criteria

- Every application has a scorecard (checks, weighted score, level) computed from
  existing model/metric/ownership signals, with history for trends.
- Scorecards aggregate by team and surface on the application detail and Insights
  dashboard; checks are configurable with shipped defaults.
- The scoring engine is pure and unit-tested on both engines with mocked storage.

## Dependencies

Milestones 31/33 (metrics + derived signals), 37 (teams/ownership), 32/35
(dashboard + trends/treemap), 08 (model). Feeds M46 (auto-remediation triggers on
failing checks) and M44 (XP for raising scores).

## Out of scope

Custom DSL/Rego-style policy languages (built-in evaluators + params only — that
generality is M-policy territory), SLA/SLO enforcement, and blocking deploys on
score — scorecards inform and trigger remediation, they do not gate releases here.
