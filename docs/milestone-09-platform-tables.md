# Milestone 09 — Platform section: tables, filters & detail pages

## Goal

Make the collected platform model browsable. Build the **Platform** section's
tabular views — applications, infrastructure, libraries, users, groups — each
with search/filtering, pagination, and a detail page that drills into one entity
and its relationships.

## Scope

- Read/query API over the platform model with filtering, sorting, pagination.
- List views for applications, infrastructure, libraries, users, groups.
- Detail pages with related entities.
- Reusable query building and a consistent table UI.

## Deliverables

### Query layer

- Read-side query traits (e.g. `PlatformQuery`) returning typed view models, kept
  separate from the write-side repositories used by the job.
- A reusable `ListQuery` parameter struct (search term, filters, sort, page,
  page size) so endpoints stay within the four-parameter rule.
- A shared `Page<T>` result type (items + total + page info) used by all lists.
- Filtering pushed into SQL (parameterised) — no in-memory full scans.

### List endpoints & views

- Applications: name, type, primary language, repository, counts of
  libs/infra/dependencies; filter by type/language/account; search by name.
- Infrastructure: name, kind, version, number of dependent applications.
- Libraries: name, ecosystem, version count, number of using applications;
  filter by ecosystem; search by name.
- Users / Groups: name, group memberships / member counts, app access counts.
- Each renders with the shared table component (server-rendered + jQuery for
  search/sort/paginate calling the JSON API).

### Detail pages

- Application detail: metadata, languages, libraries (+versions, scope),
  infrastructure, app dependencies (in/out), access grants (users/groups +
  level), and a link to the graph view (M10) centred on this app.
- Infrastructure detail: which applications use it and how.
- Library detail: versions and which applications use each version.
- User / Group detail: memberships and accessible applications with levels.

### Shared UI

- One table partial (columns, search box, sort headers, pager) reused across all
  list views — no copy-paste per entity.

## Tasks

- [ ] `PlatformQuery` read traits + sqlx impls + mocks; `ListQuery` and `Page<T>`.
- [ ] List API endpoints for each entity with filter/sort/paginate.
- [ ] List pages using a shared table partial + jQuery data loading.
- [ ] Detail API + detail pages for each entity with related data.
- [ ] Unit tests: query/filter/pagination assembly with mocked stores; view-model
      mapping helpers.

## Acceptance criteria

- Each entity type has a working list view with search, filtering, sorting, and
  pagination served from the API.
- Detail pages show an entity and all its relationships, with working
  cross-links (app → libraries/infra/users; library → apps; etc.).
- Table and query logic are unit-tested with mocks; SQL filtering is
  parameterised (no injection, no full scans).

## Dependencies

Milestone 08 (platform model populated).

## Out of scope

The graph visualisation (M10).
