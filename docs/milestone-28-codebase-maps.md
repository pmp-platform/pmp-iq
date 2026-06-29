# Milestone 28 — Auto-generated interactive codebase maps

## Goal

For each application, auto-generate an **interactive codebase map**: a zoomable
graph of the repository's internal structure — directories / modules / files and
the dependency (import/require) edges between them — browsable on the application
detail page, cross-linked to the File Explorer (M17) and to the components / use
cases attributed to those files (M17 file attribution). It answers "how is this
repo organised and what depends on what inside it?" without reading every file.

## Scope

- An analysis pass that extracts an intra-repository structure + dependency graph
  from the cloned checkout (directories/modules/files as nodes, import edges).
- A per-application persisted graph, recreated per sync (delete-and-recreate,
  consistent with the other sub-entities).
- A "Codebase Map" tab rendering the graph (reuse the AntV G6 renderer used by
  the platform graph), with directory-level default + lazy file-level drill-down.
- Cross-links: node → File Explorer path, node → attributed components/use cases.

## Deliverables

### Extraction (analysis pass)

Extend the analyzer (M08, which already inspects the checkout):

- Combine **deterministic import parsing** where cheap (language import/require
  statements, manifest module lists) with **LLM summarisation** of each
  module/directory's responsibility — so nodes carry a short description, not just
  a path.
- Produce nodes at two levels: **directory/module** (the default map) and **file**
  (loaded on demand for a selected directory). Edges are intra-repo dependencies
  (A imports B), de-duplicated and aggregated to the directory level for the
  overview.
- **Bound and surface truncation**: cap node/edge counts and depth; when capped,
  say so (consistent with the connection-graph truncation philosophy). Skip
  `.git`, vendored, and build directories (reuse the File Explorer's ignore set).

### Persistence

Per-application, recreated each sync (CASCADE with the application):

```sql
-- migrate:up
CREATE TABLE codebase_nodes (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    path           TEXT NOT NULL,           -- repo-relative dir or file
    kind           TEXT NOT NULL,           -- directory | file
    description    TEXT,                    -- LLM module summary (optional)
    UNIQUE (application_id, path)
);
CREATE TABLE codebase_edges (
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    from_path      TEXT NOT NULL,
    to_path        TEXT NOT NULL,
    PRIMARY KEY (application_id, from_path, to_path)
);
-- migrate:down
```

A dual-engine `CodebaseMapRepository` (read for the UI; the writer recreates
nodes/edges per sync alongside the existing sub-entity writes).

### UI

- A **"Codebase Map"** tab in `assets/platform-app-detail.js` +
  `templates/platform_app_detail.html`, rendering the directory-level graph with
  the existing G6 setup (zoom/reset controls, no wheel-zoom — matching the other
  diagrams).
- **Drill-down**: clicking a directory expands its file-level subgraph (lazy load
  via a read endpoint). Node colour/size encodes kind / fan-in.
- **Cross-links**: a node links into the **File Explorer** (M17) at its path and
  lists the **components / use cases** attributed to files under it (M17 file
  attribution) — turning the map into a navigation surface over the model.
- Read API: `GET /api/platform/applications/:id/codebase-map` (overview) and a
  `?dir=` variant for lazy file-level children.

## Tasks

- [ ] Analysis pass: structure + import graph (deterministic + LLM summary),
      directory/file levels, bounded with surfaced truncation.
- [ ] `codebase_nodes` / `codebase_edges` migrations + writer (recreate per sync)
      + `CodebaseMapRepository`.
- [ ] Read endpoints (overview + lazy directory drill-down).
- [ ] "Codebase Map" tab (G6) with drill-down + cross-links to File Explorer and
      attributed entities.
- [ ] Unit tests (mocked fs/analyzer/repo): graph building + truncation; directory
      aggregation of file edges; writer recreate; ignore set applied.

## Acceptance criteria

- Each application has an interactive, zoomable codebase map showing its
  directory/module structure and internal dependencies, with file-level
  drill-down.
- Map nodes link into the File Explorer and to the components/use cases attributed
  to their files.
- The map is recreated on each sync; oversized repos truncate visibly rather than
  silently; building is unit-tested with mocked dependencies.

## Dependencies

Milestones 08 (analyzer + checkout), 10 (G6 graph renderer), 17 (File Explorer +
file attribution), 09 (application detail tabs).

## Out of scope

Call-graph / symbol-level analysis (function-to-function), runtime/dynamic
dependency capture, and editing the map. This is a static structural map derived
per sync.
