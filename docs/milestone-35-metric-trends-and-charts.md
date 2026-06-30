# Milestone 35 — Metric trends & charts

## Goal

Deliver the **time-series and chart visualisations** the dashboard (M32)
described but did not fully implement: the current Insights page renders only
rollup tiles and HTML tables, and although metric **history** is stored
(`application_metrics.collected_at`), nothing surfaces trends. This milestone
vendors a chart library, exposes a series read layer, and turns the dashboard
into a visual, drill-downable view (trends, distributions, treemap, scatter), with
per-application sparklines.

## Scope

- A `series` aggregation over the M31/M33 metric history (platform-wide and
  per-application, per dimension).
- A **vendored** chart library in `assets/vendor/` (no CDN — same rule as
  G6/Mermaid).
- Dashboard charts replacing/augmenting the current tables; per-app sparklines.
- Drill-down from any chart element to the matching filtered list/detail.

## Deliverables

### Series read layer

`PlatformQuery` (or a dashboard aggregation module) gains history-aware reads,
engine-dispatched so they run on Postgres and SQLite:

- `series(metric, dimension?, window)` — average of a metric over time, optionally
  grouped (by `app_type`, `primary_language`, team/owner), from the timestamped
  history.
- `app_series(application_id, metric, window)` — one application's metric over
  time for sparklines.
- `distribution(metric, buckets)` — fleet histogram for a metric at the latest
  point.

Every figure carries the filter that produced it (reusing M09 facets) so the UI
can link through.

### Charts (vendored, no CDN)

Vendor a small charting lib (e.g. AntV G2 — consistent with the vendored G6 — or
uPlot/Chart.js) into `assets/vendor/` with a documented refresh command. Dashboard
panels:

- **Trend lines** — avg coverage / complexity / pass-rate over time, overall and
  per dimension.
- **Group-by bars** — replace the current coverage-by-language/type **tables** with
  horizontal bars.
- **Distribution histograms** — coverage and complexity spread across the fleet.
- **Scatter / bubble** — coverage (x) vs complexity (y), bubble size = LOC, to spot
  "large, complex, untested" apps.
- **Treemap** — one tile per app, size = LOC, colour = coverage (red→green) for an
  at-a-glance portfolio view.

### Per-application sparklines

The application Insights panel renders a sparkline per metric from `app_series`,
with the latest value and delta vs the previous collection.

### Drill-down

Clicking a bar/point/treemap tile navigates to the filtered entity list or the
application detail. Empty-history states render a clear "no trend yet" placeholder.

## Tasks

- [ ] `series` / `app_series` / `distribution` aggregation over metric history,
      both engines, allowlisted dimensions.
- [ ] Vendor a chart library into `assets/vendor/`; document refresh; no runtime
      CDN.
- [ ] Dashboard charts: trend lines, group-by bars, histograms, scatter/bubble,
      treemap; drill-down from each.
- [ ] Per-app sparklines + latest/delta in the Insights panel.
- [ ] Unit tests (aggregation-level, fixed dataset): series returns ordered points
      per window/dimension; distribution buckets correctly; allowlisted dimensions
      enforced; empty-history safe.

## Acceptance criteria

- The dashboard shows metric **trends over time** and fleet visual summaries
  (distribution, scatter, treemap), not just tables.
- Per-application metrics render sparklines with a delta from history.
- All charts use a locally vendored library (no CDN); aggregations run on both
  engines and are unit-tested on a fixed dataset; every figure drills down.

## Dependencies

Milestones 31/33 (metric history + categories), 32 (dashboard page + rollups/
leaderboards/groups), 09/10 (facets, filtered lists, detail pages).

## Out of scope

Configurable/custom dashboards and saved views, alerting/SLOs on trends, and
export to external BI — those are separate concerns; this is the built-in
visual dashboard.
