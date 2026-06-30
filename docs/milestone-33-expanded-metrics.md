# Milestone 33 — Expanded metrics catalog (LLM-sourced)

## Goal

Broaden the per-application metric set well beyond the M31 core seven
(`tests_total`/`passed`/`failed`, `coverage_pct`, `complexity_avg`, `loc`,
`has_ci`) to cover **code health, security/supply-chain, delivery, ownership,
architecture, and platform-model coverage**. Every new metric is **collected
with the LLM for now** (additive to the existing M31 store), so the dashboard
(M32) and trends (M35) have far richer signal without new infrastructure. Later
milestones can swap individual collectors for dedicated scanners/provider APIs
without changing the store or the read layer.

## Scope

- New metric **keys**, organised into named **categories** (code-health,
  security, delivery, ownership, architecture, model-coverage).
- A metric **registry** describing each metric (key, label, unit, category,
  `higher_is_better`) so the dashboard/leaderboards know how to render and rank it.
- Collection by extending the `collect-metrics` job (M31) with one bounded LLM
  pass per category, normalised through the existing `parse_metrics` pattern
  (omit nulls, never fabricate), recorded via the recorder.
- The additive, history-keeping `application_metrics` store is reused unchanged
  in shape — new metrics are just new keys carrying a `category`.

## Deliverables

### Metric registry

A static, code-owned registry — the single source of truth mapping each metric
**key** to its **category** (`src/metrics/registry.rs`). The repository stamps
every row's category from here at write time, so the Insights panel and dashboard
group/theme metrics without per-metric UI code:

```rust
pub enum MetricCategory { CodeHealth, Security, Delivery, Ownership, Architecture, ModelCoverage, General }
pub fn category_for(key: &str) -> MetricCategory;   // defaults to General for unregistered keys
```

(Ranking metadata — friendly labels and `higher_is_better` for leaderboards —
lands with the charts/leaderboards work in M35, keeping this registry to exactly
what M33 uses.)

### Expanded LLM metric set (collected for now)

Each category is a bounded JSON object the LLM emits from the checkout/CI; absent
fields are omitted (additive). Representative keys:

- **Code health:** `duplication_pct`, `lint_warnings`, `todo_count`,
  `doc_coverage_pct`, and **convention compliance** (`fns_over_50_lines`,
  `files_over_1000_lines`, `fns_over_4_params`) — the project's own standards,
  self-applied.
- **Security / supply chain:** `vuln_critical|high|medium|low`,
  `deps_outdated`, `max_dep_age_days`, `secrets_detected`, `license_risk`,
  `dependency_count`.
- **Delivery:** `pr_merge_time_hours`, `open_pr_count`, `pr_avg_size_loc`
  (from provider PR data where available; LLM-estimated otherwise).
- **Ownership:** `bus_factor`, `has_codeowners`, `orphaned` (no members).
- **Architecture:** `fan_in`, `fan_out`, `external_dependency_count` (derivable
  from the platform graph — computed, not prompted, where possible).
- **Model coverage (computed, free):** `has_use_cases`, `has_diagrams`,
  `observability_signal_count`, `sync_age_days`.

Architecture and model-coverage metrics that are derivable from the catalog/graph
are **computed in Rust** (not prompted) and recorded with `source = "derived"`;
the rest are `source = "llm"`.

### Persistence

Additive migration only — extend the M31 table:

```sql
-- migrate:up
ALTER TABLE application_metrics ADD COLUMN category TEXT NOT NULL DEFAULT 'general';
CREATE INDEX idx_application_metrics_category ON application_metrics(application_id, category);
-- migrate:down
```

`ApplicationMetricsRepository::record` gains the category (defaulted from the
registry by key); `latest_for_application` / `latest_all` are unchanged.

### UI

The application **Insights** panel groups metrics by category with the registry
label + unit and a per-metric source badge (`llm` / `derived`). New metrics flow
into the M32 dashboard automatically via the registry.

## Tasks

- [ ] `MetricDef` registry + `MetricCategory`; seed all new keys with label/unit/
      direction.
- [ ] Per-category LLM prompt parts (bounded JSON; omit-nulls) wired into
      `collect-metrics`; recorded.
- [ ] Computed (derived) metrics for architecture/model-coverage from the graph/
      catalog — no LLM.
- [ ] `category` migration (both engines) + repository wiring.
- [ ] Insights panel grouped by category; dashboard picks up new metrics via the
      registry.
- [ ] Unit tests (mocked AI/repo/graph): each category prompt normalises a sample
      response into the expected keyed metrics; nulls omitted; derived metrics
      computed from a fixed graph; category persisted.

## Acceptance criteria

- Applications carry a substantially expanded, categorised metric set; LLM-sourced
  metrics are normalised and never fabricated; derived metrics are computed from
  the catalog.
- New metrics appear in the per-app Insights panel and the dashboard without
  bespoke per-metric UI (registry-driven).
- Collection and derivation are unit-tested with mocked dependencies (no live CI).

## Dependencies

Milestones 31 (metrics store + `collect-metrics` job + recorder), 08/10
(analyzer + graph), 32 (dashboard consumes the registry). Pairs with M34
(configurable prompts) and feeds M35 (trends/charts).

## Out of scope

Dedicated security scanners / SBOM tools, real DORA pipelines, and any non-LLM
external integration — those replace individual collectors in later milestones;
here every non-derived metric is LLM-sourced.
