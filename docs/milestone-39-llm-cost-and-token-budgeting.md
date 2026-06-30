# Milestone 39 — LLM cost & token budgeting

## Goal

Turn the token usage already recorded per execution (the `RecordingAiProvider`
accumulates token counts into execution metadata) into **cost** — priced per
model — aggregated by job, application, AI profile, and time period, and enforce
**budgets** that warn or stop work when a configured limit is exceeded. LLM spend
is the main running cost of pmp-iq; this makes it visible and controllable.

## Scope

- A queryable usage/cost rollup over recorded token usage.
- A model → price map (configurable, updatable) to convert tokens to cost.
- Budgets per AI profile / job / application with warn and hard-stop thresholds.
- A cost panel in Insights and per-execution cost; budget enforcement in the job
  layer.

## Deliverables

### Usage capture (queryable)

The recorder already writes token counts to execution metadata; to make spend
queryable without scanning metadata, it also appends a row per LLM call to a
dedicated table:

```sql
-- migrate:up
CREATE TABLE llm_usage (
    id               UUID PRIMARY KEY,
    job_execution_id UUID NOT NULL,
    application_id   UUID,            -- when the call is app-scoped
    ai_profile_id    UUID,
    model            TEXT NOT NULL,
    input_tokens     BIGINT NOT NULL,
    output_tokens    BIGINT NOT NULL,
    occurred_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_llm_usage_app ON llm_usage(application_id, occurred_at DESC);
CREATE INDEX idx_llm_usage_profile ON llm_usage(ai_profile_id, occurred_at DESC);
-- migrate:down
```

Dual-engine `LlmUsageRepository`. Metadata recording is kept for the live
execution view; the table backs aggregation.

### Pricing

A configurable `model → { input_per_mtok, output_per_mtok }` map (in
`config.yaml`/env, seeded with current Claude model prices, defaulting to a model
when unknown). A pure `cost(usage, prices)` converts tokens → cost so it is
unit-testable and prices can be updated without code.

### Budgets & enforcement

```sql
-- migrate:up
CREATE TABLE llm_budgets (
    id          UUID PRIMARY KEY,
    scope       TEXT NOT NULL,     -- profile | job | application | global
    scope_id    UUID,              -- null for global
    period      TEXT NOT NULL,     -- monthly | daily
    limit_usd   DOUBLE PRECISION NOT NULL,
    hard_stop   BOOLEAN NOT NULL DEFAULT FALSE
);
-- migrate:down
```

Before an LLM-using job runs (and between turns for long agent tasks), a
`BudgetGuard` sums period-to-date cost for the relevant scopes. Over a **warn**
threshold it logs/annotates the execution; over a **hard-stop** budget it returns
`JobError::CannotRun { retry_at: <next period> }` (consistent with the existing
rate-limit self-pause), so the controller reschedules rather than failing.

### UI

- A **Cost** panel in Insights: spend by period, by application, by profile, by job
  type; top spenders; projected month-end.
- Per-execution **cost** shown on the job execution page (alongside tokens).
- Budgets managed in Settings; current period-to-date vs limit shown with a
  warn/over indicator.

## Tasks

- [ ] `llm_usage` migration + dual-engine repository; recorder appends a usage row
      per call (keeping metadata token recording).
- [ ] Configurable model price map + pure `cost()`; default-on-unknown-model.
- [ ] `llm_budgets` + `BudgetGuard`: period-to-date sum per scope, warn annotate,
      hard-stop → `CannotRun{retry_at}`.
- [ ] Cost panel in Insights + per-execution cost; budget management in Settings.
- [ ] Unit tests (mocked repo/clock): usage rows aggregate to expected cost per
      scope/period; `cost()` math; warn annotates, hard-stop reschedules; unknown
      model falls back to default price.

## Acceptance criteria

- Recorded token usage is converted to cost via a configurable per-model price map
  and aggregated by job/application/profile/period.
- Budgets warn and (when hard-stop) reschedule work past the limit via the existing
  `CannotRun` path; never silently overspend.
- Spend is visible per execution and in an Insights cost panel; aggregation and
  enforcement are unit-tested on both engines with mocked dependencies.

## Dependencies

Milestones 05 (AI profiles + model), 13 (recorder + execution metadata + tokens),
06 (`JobError::CannotRun`/reschedule), 32 (Insights surface). Pairs with M33/M35
(cost as a metric/trend).

## Out of scope

Real-time provider billing reconciliation, prepaid credit management, and
per-request rate shaping — a usage-derived cost model + budget guard.
