# Milestone 31 — LLM-collected quality metrics (tests, coverage, complexity)

## Goal

Collect per-application quality metrics by having the analysis/agent inspect both
**CI/Actions results** and the **codebase**: number of tests run, pass vs fail,
coverage %, and code complexity (plus LOC/file counts). The LLM normalises
heterogeneous CI output and coverage/test report formats into a uniform,
queryable metric set stored on the platform model — feeding the dashboard (M32)
and drift/trends.

## Scope

- Gathering metrics from two sources per repository: CI/Actions (test counts,
  pass/fail, coverage artifacts) and the checkout (complexity, LOC, test presence).
- An LLM normalisation step that turns varied CI logs / junit / coverage reports
  into structured `{ key, value, unit }` metrics.
- A flexible, additive metrics store with history (so trends are possible).
- A collection job behind the per-repo lock; metrics shown per application.

## Deliverables

### Sources

- **CI / Actions** — via the provider: list the latest workflow run on the default
  branch, its check/job conclusions, and downloadable artifacts/logs. Extend
  `RepositoryProvider` with `latest_ci_run(repo, branch)` →
  `{ status, conclusion, jobs, artifacts }` and `ci_artifact(...)` /
  `ci_logs(...)` (GitHub Actions API; `Unsupported` default elsewhere).
- **Checkout** — the analyzer (M08) computes/extracts complexity (cyclomatic /
  maintainability), LOC, file/function counts, and whether tests exist, using a
  language-appropriate tool where available plus LLM estimation otherwise.

### LLM normalisation

The differentiator: CI output and coverage/test reports vary wildly
(junit XML, `lcov`, `coverage.xml`, `go test`, `cargo test`, pytest summaries,
plain logs). An **LLM normalisation pass** reads the available
artifacts/logs/reports (bounded, via the recorder) and emits a structured set:

```json
{ "tests_total": 412, "tests_passed": 408, "tests_failed": 4,
  "coverage_pct": 83.5, "complexity_avg": 7.2, "loc": 21450, "has_ci": true }
```

Unknown/absent metrics are simply omitted (additive, never fabricated — the
prompt requires "report only what the evidence supports").

### Persistence

Flexible key/value with history (recreated/appended per sync, timestamped so M32
can show trends):

```sql
-- migrate:up
CREATE TABLE application_metrics (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    metric_key     TEXT NOT NULL,     -- tests_total | coverage_pct | complexity_avg | ...
    value          DOUBLE PRECISION NOT NULL,
    unit           TEXT,              -- count | percent | ratio
    source         TEXT NOT NULL,     -- ci | codebase
    collected_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_application_metrics_app ON application_metrics(application_id, metric_key, collected_at DESC);
-- migrate:down
```

A dual-engine `ApplicationMetricsRepository` (write the latest set per sync;
keep history for trends; read the latest + a series).

### Collection job & UI

- A `collect-metrics` job (or a metrics pass within `sync-repositories`, M08) that,
  per selected repo and behind the per-repo lock, gathers CI + codebase metrics,
  runs the LLM normalisation (recorded), and writes the metric set.
- The application detail gains an **"Insights / Metrics"** panel: tests
  passed/failed, coverage, complexity, LOC, "has CI" — with the source and
  `collected_at`, and a small sparkline when history exists.

## Tasks

- [ ] Provider CI methods (`latest_ci_run`/`ci_artifact`/`ci_logs`) + GitHub impl
      + mocks; `Unsupported` default.
- [ ] Analyzer codebase metrics (complexity/LOC/test presence).
- [ ] LLM normalisation pass → structured `{ key, value, unit, source }` (recorded;
      report-only-what-evidence-supports).
- [ ] `application_metrics` migration + `ApplicationMetricsRepository` (latest +
      series).
- [ ] `collect-metrics` job (per-repo lock) / metrics pass in sync; Insights panel.
- [ ] Unit tests (mocked provider/analyzer/AI/repo): junit + lcov samples normalise
      to the expected metrics; absent CI yields `has_ci=false` and omits CI metrics;
      nothing is fabricated; metrics persist with source + timestamp.

## Acceptance criteria

- Each application has uniform quality metrics — test count, pass/fail, coverage,
  complexity, LOC — collected from its CI and codebase, with the source and time.
- Heterogeneous CI/coverage formats are normalised by the LLM into the same metric
  keys; missing data is omitted, not invented.
- Metrics are stored with history and shown per application; collection is
  unit-tested with mocked dependencies (no live CI).

## Dependencies

Milestones 08 (analyzer + checkout), 05/13 (AI providers + recorder), 12 (per-repo
lock), 07 (repositories/accounts + provider). Feeds M32.

## Out of scope

Running the test suite ourselves (we read existing CI results), real-time CI
streaming, and language-specific deep static analysis beyond standard
complexity/LOC. The dashboard/aggregation is M32.
