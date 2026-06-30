# Milestone 37 — RBAC, teams & multi-tenant

## Goal

Move beyond the current single-admin / GitHub-allowlist auth to **roles**,
**teams**, and optional **tenant isolation**. Define roles (admin / maintainer /
viewer), teams that **own applications**, and per-team scoping of views and
actions; optionally isolate all data per tenant (organisation). This turns
pmp-iq from a single-operator tool into something a platform org can share, and
makes the M32 "group by team/owner" dimension first-class.

## Scope

- A roles + permissions model and `require_role`/permission middleware layered on
  the existing `require_auth`.
- Teams, team membership, and team → application ownership (seedable from
  `access_grants` codeowners/members).
- Scoped reads/writes: viewers read; maintainers act on owned apps; admins do
  everything.
- Optional, feature-flagged **multi-tenant** isolation via a `tenant_id` scope on
  the principal and data.

## Deliverables

### Roles & permissions

```sql
-- migrate:up
CREATE TABLE roles (
    principal TEXT PRIMARY KEY,    -- username / oauth login
    role      TEXT NOT NULL        -- admin | maintainer | viewer
);
-- migrate:down
```

`Principal` (M03) gains a `role`; a `require_role(min)` / `require_permission(p)`
middleware wraps protected routes. A single first-admin bootstrap keeps the
current admin working. Permissions map actions (run jobs, create agent tasks/
campaigns, edit settings/prompts, manage teams) to the minimum role.

### Teams & ownership

```sql
-- migrate:up
CREATE TABLE teams (
    id   UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);
CREATE TABLE team_members (
    team_id   UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    principal TEXT NOT NULL,
    PRIMARY KEY (team_id, principal)
);
CREATE TABLE team_applications (
    team_id        UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    PRIMARY KEY (team_id, application_id)
);
-- migrate:down
```

Dual-engine `TeamRepository`. A seeding helper proposes ownership from existing
`access_grants` (codeowners/members) so teams aren't empty on day one. The M32
dashboard's owner/team group-by now keys on real teams.

### Scoped access

- **Viewer:** read-only across the catalog (all apps).
- **Maintainer:** read all; write (sync, agent tasks, campaigns, hints, prompts
  scoped to settings) only on applications their team owns.
- **Admin:** everything, incl. team/role management and settings.

Read/write queries take an optional principal scope; the route layer enforces the
role/ownership check and returns `403` on violation. The application detail shows
its owning team(s); agent-task/campaign/sync actions are gated by ownership.

### Multi-tenant (optional, feature-flagged)

When `multitenant.enabled`, a `tenant_id` is attached to the principal (e.g. from
the GitHub org) and added to the major tables; every query is scoped to the
caller's tenant, isolating catalogs per organisation. Disabled by default —
single-tenant behaviour is unchanged.

## Tasks

- [ ] `roles` migration + `Principal.role` + `require_role`/`require_permission`
      middleware; admin bootstrap preserved.
- [ ] `teams`/`team_members`/`team_applications` migrations + dual-engine
      `TeamRepository`; ownership seeding from `access_grants`.
- [ ] Ownership-scoped read/write enforcement at the route layer (`403` on
      violation); owning team shown on app detail; gated actions.
- [ ] Optional `tenant_id` scoping behind a config flag; all queries tenant-scoped
      when enabled.
- [ ] Team management UI in Settings; dashboard group-by-team uses real teams.
- [ ] Unit tests (mocked repos/middleware): role gating allows/denies the right
      actions; a maintainer can act only on owned apps; tenant scope hides other
      tenants' data; admin bootstrap still works.

## Acceptance criteria

- Roles gate actions (viewer/maintainer/admin); maintainers can only mutate apps
  their team owns; admins manage teams and roles.
- Teams own applications (seedable from existing access grants) and drive the
  dashboard's team breakdowns.
- Optional tenant isolation scopes all data when enabled; everything is unit-tested
  with mocked auth/storage and both engines.

## Dependencies

Milestones 03 (auth/login strategies, `require_auth`, `Principal`), 21 (GitHub
login → org for tenant/team seeding), the membership/`access_grants` model, 32
(team dimension), 36 (audit gains actor roles).

## Out of scope

A full external IdP/SCIM/SAML integration, fine-grained per-field ACLs, and
cross-tenant sharing — a pragmatic role + team + optional tenant scope.
