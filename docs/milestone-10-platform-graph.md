# Milestone 10 — Platform section: connection graph

## Goal

Add the visual centrepiece: an interactive **graph** of the platform showing
applications and how they connect to each other and to infrastructure, with
drill-down from a node into the entity's detail (M09). The graph library is
served locally.

## Scope

- A graph data API producing nodes and edges from the platform model.
- A client-side interactive graph rendered with a locally-served library.
- Filtering/scoping (by account, app type, infra kind, or centred on one app).
- Drill-down: click a node → entity detail / focused subgraph.

## Deliverables

### Graph data API

- `GraphQuery` read trait building a graph view model:
  - **Nodes**: applications, infrastructure (and optionally groups), each with
    id, type, label, and summary attributes for styling/tooltips.
  - **Edges**: `application_dependencies` (app→app) and
    `application_infrastructure` (app→infra), typed by `kind`.
- A `GraphScope` parameter struct (center node, depth, filters) keeps the
  endpoint within the parameter limit and is reused by the focused view.
- Output is a stable JSON shape decoupled from the storage schema:
  `{ nodes: [...], edges: [...], truncated: bool, total_applications: int }`.
  `truncated` is `true` when the total application count exceeds the scope
  limit (so the client can show a "not all nodes shown" notice), and
  `total_applications` reports the full count regardless of truncation.

### Visualisation

- Vendor a graph library locally into `assets/vendor/` (e.g. **Cytoscape.js** or
  **vis-network**) — no CDN at runtime; document the refresh procedure.
- Platform → Graph page: renders the full graph with:
  - Node colour/shape by type (app type, infrastructure kind).
  - Edge styling/labels by dependency `kind`.
  - Zoom/pan, a legend, and hover tooltips.
- Controls (jQuery): filter by account / app type / infra kind; search-to-focus a
  node; toggle infrastructure nodes.

### Drill-down

- Clicking a node opens its M09 detail (or a side panel) and offers "focus" to
  re-query a subgraph centred on that node at a chosen depth via `GraphScope`.

### Performance

- For large graphs, cap returned nodes/edges with server-side scoping and
  **log/surface** when results are truncated (never silently). Provide focus +
  filters as the path to detail rather than rendering everything at once.

## Tasks

- [ ] `GraphQuery` trait + sqlx impl + mock; `GraphScope`; JSON node/edge shape.
- [ ] Graph data endpoints (full + focused/scoped).
- [ ] Vendor the graph library locally; add the Graph page and rendering code.
- [ ] Filters, search-to-focus, infra toggle, legend, tooltips (jQuery).
- [ ] Node click → detail/side panel + focus action.
- [ ] Unit tests: node/edge assembly and scoping/truncation logic with mocked
      store; truncation is reported, not hidden.

## Acceptance criteria

- The Graph page renders applications and their connections to other apps and
  infrastructure, loaded entirely from local assets.
- Filtering and focusing work; clicking a node drills into its detail and can
  re-centre the graph on it.
- Large result sets are scoped/truncated with a visible notice; assembly logic is
  unit-tested with mocks.

## Dependencies

Milestones 08 (model) and 09 (detail pages).

## Out of scope

Editing the model from the graph (read-only visualisation).
