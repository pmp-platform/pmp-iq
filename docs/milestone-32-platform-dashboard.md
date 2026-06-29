# Milestone 32 — Platform metrics & insights dashboard

## Goal

A cross-repository **dashboard** that turns the quality metrics (M31) and the
catalog into platform-level insight: rollups, **group-by** breakdowns across many
dimensions (team/owner, application type, language, ecosystem, kind), and
**leaderboards** (top applications by coverage, lowest complexity, best test pass
rate, and other platform-worthy metrics). It is the "how healthy is our platform"
view that a platform or engineering-leadership team checks regularly.

## Scope

- Aggregate queries over `application_metrics` (M31) + the catalog, grouped by the
  existing facet dimensions.
- A dashboard page: rollup tiles, group-by charts/tables, leaderboards, and
  (when history exists) trends.
- Drill-down from any figure to the filtered entity list / detail pages.

## Deliverables

### Aggregation layer

`PlatformQuery` gains read-only **aggregation** methods (engine-dispatched, so
they work on Postgres and SQLite):

- `rollup()` — platform totals + averages (apps, libraries, infra; avg coverage,
  avg complexity, overall pass rate, % apps with CI, % apps with an owner).
- `group_metrics(dimension, metric)` — a metric aggregated by a dimension
  (team/owner, `app_type`, `primary_language`, library `ecosystem`, entity `kind`),
  reusing the allowlisted facet fields (M09 `filter_fields`/`facets`).
- `leaderboard(metric, order, limit)` — top/bottom N applications by a metric.
- `series(metric, dimension?)` — a time series from the M31 history for trends.

All return compact, typed results; every figure carries the filter that produced
it so the UI can link to the matching entity list.

### Dashboard page & metrics

A new **"Insights"** dashboard (the platform section's landing view or a dedicated
tab), rendered with a **vendored** chart library (downloaded into `assets/vendor/`,
no CDN — same rule as G6/Mermaid) plus the existing local-table for tabular
breakdowns. Suggested platform-worthy panels:

- **Rollup tiles**: total applications / libraries / infrastructure; average
  coverage and complexity; overall test pass rate; % of apps with CI; % with an
  owner; count of apps with **no tests** and apps on **EOL/outdated** library
  versions.
- **Group-by breakdowns** (bar/heatmap): coverage and complexity **by team/owner**,
  **by application type**, **by language/ecosystem** — to spot which areas lag.
- **Leaderboards**: top apps by coverage, lowest average complexity, highest test
  pass rate; and the inverse "needs attention" lists (lowest coverage, highest
  complexity, most failing checks).
- **Dependency insight**: most-depended-on libraries/services (fan-in), apps with
  the largest dependency fan-out, and **risk** (apps depending on EOL libs or
  failing-CI services).
- **Activity**: open agent tasks / campaign PRs, merge throughput (from M22–M30),
  and architectural **drift** events (new infra/dependencies since last sync).
- **Trends** (when M31 history exists): coverage/complexity/pass-rate over time,
  per dimension.

Every tile/row drills down to the relevant filtered entity list or detail page.

### Ownership note

Several breakdowns key on **team/owner**. If ownership isn't modelled yet, this
milestone introduces a minimal owner dimension (e.g. derive from `access_grants`
codeowners/members, or a per-application `owner` field) so group-by-team works;
full team management can be a later milestone.

## Tasks

- [ ] `PlatformQuery` aggregation methods (`rollup`/`group_metrics`/`leaderboard`/
      `series`) over `application_metrics` + catalog, allowlisted dimensions.
- [ ] Vendor a chart library into `assets/vendor/`; document the refresh procedure.
- [ ] Insights dashboard: rollup tiles, group-by charts, leaderboards, dependency
      insight, activity, trends; each figure drills down to a filtered list.
- [ ] Minimal owner/team dimension (from `access_grants` or an `owner` field).
- [ ] Unit tests (mocked/aggregation-level): rollup/group/leaderboard return
      expected aggregates for a fixed dataset; dimensions are allowlisted (an
      unknown group-by is rejected); empty-data states are handled.

## Acceptance criteria

- A dashboard shows platform rollups, metric breakdowns grouped by team/type/
  language/ecosystem, and top/bottom leaderboards (coverage, complexity, pass
  rate, dependencies).
- Every figure links to the matching filtered entity list/detail; trends render
  when metric history exists.
- Aggregations run on both database engines through the query layer and are
  unit-tested against a fixed dataset.

## Dependencies

Milestones 31 (quality metrics + history), 09/10 (catalog, facets/filters, detail
pages), 22–30 (activity/campaign signals). Optional ownership ties to
`access_grants` (M-membership).

## Out of scope

Configurable/custom dashboards and saved views, alerting/SLOs on metrics, and
exporting to external BI tools — a fixed, opinionated platform dashboard for now.
