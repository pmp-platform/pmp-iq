# Milestone 45 — Version currency & tech radar

## Goal

Make the fleet's **technology currency** visible: how far behind each application's
languages, frameworks and libraries are, which runtimes are **end-of-life**, and
where the organisation stands on each technology via a **tech radar**
(adopt / trial / assess / hold across language / framework / infrastructure / tool
quadrants). The platform already catalogues languages (M08), libraries with
versions (`library_versions`), and linked infrastructure/tools/platforms; this
milestone layers a currency/EOL policy over them and turns adoption counts into a
radar, so "stop starting new services on X" and "Y is EOL in 60 days" become
data-driven.

## Scope

- A configurable **version policy** per ecosystem/technology: latest-known
  version and EOL date (seeded, updatable from config or an import).
- A pure **currency** calculation: how many versions / how stale each app's deps
  are, and which are EOL or EOL-soon.
- A **tech radar**: ring (adopt/trial/assess/hold) + quadrant per technology,
  operator-curated and/or derived from fleet adoption + currency.
- Per-app outdated-dependency reports, a fleet currency rollup, and a radar page.

## Deliverables

### Policy & radar tables

```sql
-- migrate:up
-- Known-current version + EOL per technology (ecosystem-qualified for libraries,
-- bare for languages/runtimes). Seeded; editable; importable.
CREATE TABLE version_policy (
    id            UUID PRIMARY KEY,
    ecosystem     TEXT,                    -- cargo | npm | pip | ... | null for languages/runtimes
    name          TEXT NOT NULL,
    latest        TEXT,                    -- known-current version
    eol_date      DATE,                    -- end of life, if known
    UNIQUE (ecosystem, name)
);
-- Tech-radar placement (curated, optionally seeded from adoption).
CREATE TABLE tech_radar (
    id        UUID PRIMARY KEY,
    quadrant  TEXT NOT NULL,               -- language | framework | infrastructure | tool
    name      TEXT NOT NULL,
    ring      TEXT NOT NULL,               -- adopt | trial | assess | hold
    note      TEXT,
    UNIQUE (quadrant, name)
);
-- migrate:down
```

### Currency calculation

A pure module computing, per application and fleet-wide:

- **Lag** — for each library version (`library_versions`, M08) the distance to the
  policy `latest` (major/minor behind via semver parsing; "unknown" when no policy).
- **EOL status** — `current` | `eol_soon` (within a configurable window) | `eol`,
  from `version_policy.eol_date` and the platform `currentDate`-style clock.
- **Currency score** — a per-app summary (e.g. fraction of deps current,
  count EOL) recorded as a metric (M31) so it trends (M35).

Semver/version parsing and lag/EOL bucketing are unit-tested on fixed inputs.

### Radar derivation

`adopt/trial/assess/hold` is seedable from signals — broad current adoption →
`adopt`, EOL/declining → `hold`, rare/new → `assess` — then operator-overridable
via `tech_radar`. The radar reads the existing catalog (languages + linked
entities + libraries) for adoption counts.

### UI

- A **Tech radar** page (the classic four-quadrant rings view, vendored SVG like
  the M35 charts), filterable by quadrant; clicking a blip lists adopting apps.
- A **Currency** report: fleet table of EOL / outdated technologies with the
  affected applications (drill-through, M09), and a per-app "outdated dependencies"
  panel on the application detail.
- Currency score on the Insights dashboard (M32) + as a trend (M35).

## Tasks

- [ ] `version_policy` + `tech_radar` migrations (both engines) + dual-engine
      repository; seed common ecosystems/runtimes + a default radar.
- [ ] Pure currency module: semver lag, EOL bucketing, per-app/fleet rollup;
      record currency as a metric.
- [ ] Radar derivation from adoption + currency, with operator overrides.
- [ ] Tech-radar page (vendored SVG), currency report + per-app panel, dashboard
      score/trend; drill-through to affected apps.
- [ ] Unit tests (fixed data): lag/EOL bucketing per version; currency score;
      radar ring derivation; unknown-policy and missing-EOL safe.

## Acceptance criteria

- Each application reports how outdated its dependencies/runtimes are and which are
  EOL/EOL-soon, against a configurable policy; a fleet currency score trends over
  time.
- A tech radar (quadrants × rings) is viewable and drill-downable to adopting
  apps, derived from adoption + currency and operator-overridable.
- Currency and radar derivation are pure and unit-tested on both engines.

## Dependencies

Milestones 08 (languages/libraries/versions + linked entities), 09/10 (filtered
lists + drill-through), 31/35 (metric + trend), 32 (dashboard), 40 (the radar
complements semantic grouping). Feeds M43 (a "no EOL runtimes" check) and M46
(remediation: bump outdated/EOL deps).

## Out of scope

Live registry polling for the absolute newest versions (the policy is configured/
imported, not crawled at runtime), automatic dependency upgrades (that is M46
remediation), and CVE/security scoring (separate security/SBOM concern).
