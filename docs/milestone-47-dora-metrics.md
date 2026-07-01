# Milestone 47 — DORA metrics

## Goal

Measure delivery performance with the four **DORA** metrics — **deployment
frequency**, **lead time for changes**, **change-failure rate** and **time to
restore (MTTR)** — per application, per team and fleet-wide, with performance
tiers (elite / high / medium / low) and trends. The platform already ingests PR
and push events via webhooks (M25), tracks PR lifecycles (M24), records changes
(M36) and renders metric trends (M35); this milestone captures **deployment** and
**incident** events and derives the DORA measures over them.

## Scope

- Capture **deployment** events (from webhooks — GitHub `deployment`/`release`,
  or a generic `POST /events/deploy` — and/or a tag/branch convention) and
  **incident** events (open/resolve, via webhook or a small API).
- A pure **DORA computation** over the event history per app/team/window.
- Performance tiering + trends; a DORA panel in Insights and per-app/team views.

## Deliverables

### Event capture

```sql
-- migrate:up
CREATE TABLE deployments (
    id             UUID PRIMARY KEY,
    application_id UUID REFERENCES applications(id) ON DELETE CASCADE,
    environment    TEXT NOT NULL DEFAULT 'production',
    sha            TEXT,
    succeeded      BOOLEAN NOT NULL DEFAULT TRUE,
    deployed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- first commit of the change (for lead time), when resolvable.
    first_commit_at TIMESTAMPTZ
);
CREATE INDEX idx_deployments_app ON deployments(application_id, deployed_at DESC);
CREATE TABLE incidents (
    id             UUID PRIMARY KEY,
    application_id UUID REFERENCES applications(id) ON DELETE CASCADE,
    caused_by      UUID REFERENCES deployments(id) ON DELETE SET NULL,
    opened_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at    TIMESTAMPTZ
);
CREATE INDEX idx_incidents_app ON incidents(application_id, opened_at DESC);
-- migrate:down
```

The webhook handler (M25) gains `deployment`/`release` ingestion (resolving the
repository → application via the existing mapping); lead time uses the deployed
sha back to the first commit of its change via `GitClient` (M41 `changed_files`
machinery). A generic authenticated `POST /api/events/{deploy,incident}` lets CI
or external systems report events directly.

### DORA computation

A pure module over the event history:

- **Deployment frequency** — successful deployments per window.
- **Lead time for changes** — median `deployed_at − first_commit_at`.
- **Change-failure rate** — fraction of deployments that caused an incident.
- **MTTR** — median `resolved_at − opened_at` for incidents.
- **Tier** — map each metric to elite/high/medium/low by the standard bands.

All four (and the tier) are computed from fixed event sets and unit-tested; they
are recorded as metrics (M31) per app so they trend via M35 and roll up by team
(M37).

### UI

- A **DORA** panel on the Insights dashboard (M32): the four metrics + tier for
  the fleet and a team selector, with M35 trend lines per metric.
- Per-application DORA on the application detail (sparklines, M35) and a per-team
  comparison table.
- Drill-through from a metric to the underlying deployments/incidents.

## Tasks

- [ ] `deployments` + `incidents` migrations (both engines) + dual-engine
      repository; webhook deployment/release ingestion + generic event API.
- [ ] Lead-time resolution (deployed sha → first commit) via `GitClient`.
- [ ] Pure DORA computation (frequency, lead time, change-failure rate, MTTR) +
      tiering; record as per-app metrics.
- [ ] DORA dashboard panel + per-app/team views + trends; drill-through to events.
- [ ] Unit tests (fixed event sets): each metric computes correctly per window;
      tier bands map right; empty-history safe; change-failure links incident→deploy.

## Acceptance criteria

- The four DORA metrics and a performance tier are computed per application, team
  and fleet from captured deployment/incident events, and trend over time.
- Deployments arrive via webhooks and/or a generic event API; lead time resolves
  to the first commit of a change where possible.
- The DORA computation is pure and unit-tested on fixed event sets (both engines);
  metrics surface on the dashboard with drill-through.

## Dependencies

Milestones 25 (webhooks — event ingestion), 24 (PR lifecycle), 31/35 (metric +
trend), 32 (dashboard), 37 (team rollup), 41 (`changed_files`/commit resolution
for lead time), 36 (incidents complement the change feed). Pairs with M43
(delivery performance as a readiness check).

## Out of scope

Full incident-management/on-call (incidents here are just open/resolve markers for
MTTR), pulling deploy data from every CI/CD vendor's native API (webhook + generic
event API only), and SPACE/flow-metrics beyond the four DORA measures.
