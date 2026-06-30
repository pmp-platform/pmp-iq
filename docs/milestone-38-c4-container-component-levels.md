# Milestone 38 — C4 Container & Component levels

## Goal

Extend the C4 export (M29 — System-Context only) down the C4 hierarchy:
**Container** diagrams (an application's runnable/deployable units and its
datastores) and **Component** diagrams (the internal components the analyzer
already extracts), in both **Structurizr DSL** and **C4 Mermaid**. This gives a
proper zoom path: fleet **Context** → one app's **Containers** → its
**Components**, all projected deterministically from the platform model (so the
views stay current and consistent across every application).

## Scope

- Two new projections — Container and Component — alongside the existing
  System-Context, reusing the `project`/`ident`/`escape` machinery in `c4.rs`.
- Driven by data already in the model: an application's infrastructure/datastores
  and app→app edges (containers) and its `components` + `use_case_components` +
  dependency `component_id` (components).
- Page-level drill-down across the three levels; export of each as DSL + Mermaid.

## Deliverables

### Container view

For a selected application, project its **containers** = the app itself plus the
runnable/data units it owns (databases, queues, caches from its infrastructure
relations), with relationships from its outbound dependencies:

- `container_dsl(app, graph)` → Structurizr `softwareSystem { container ... }`.
- `container_mermaid(app, graph)` → `C4Container` with `Container` /
  `ContainerDb` / `Container_Ext` macros.

The app's external dependencies render as `System_Ext`/`Container_Ext` so the
container view shows boundaries cleanly.

### Component view

For a selected application, project its **components** (M08 `components`) and the
relationships between them derived from `use_case_components` and the
dependency → `component_id` mapping (which component makes each outbound call):

- `component_dsl(app)` / `component_mermaid(app)` → Structurizr `component` /
  `C4Component` with `Component` macros and `Rel` edges between components and to
  external systems they call.

### API & UI

- `GET /api/platform/c4?level=context|container|component&application=:id&dependencies=`
  returns `{ dsl, mermaid }` for the requested level (context = current fleet
  view; container/component require an application).
- The C4 page gains a **level selector** and, for container/component, an
  application picker; clicking a system in Context drills into its Container view,
  and a container into its Component view. The existing Mermaid render + DSL panel
  are reused; the `include_dependencies` toggle still applies.

## Tasks

- [ ] `container_dsl`/`container_mermaid` projecting an app's containers +
      datastores + external boundaries from the graph.
- [ ] `component_dsl`/`component_mermaid` projecting `components` with edges from
      `use_case_components` + dependency `component_id`.
- [ ] `level`/`application` params on the C4 route; per-level `{dsl, mermaid}`.
- [ ] C4 page: level selector, application picker, context→container→component
      drill-down; reuse Mermaid render + DSL panel + dependencies toggle.
- [ ] Unit tests (pure projection over fixed graphs/component sets): container view
      includes the app + its datastores and external boundaries; component view
      includes components and inter-component + component→external edges; idents
      sanitised; apps-only vs include-dependencies behave per level.

## Acceptance criteria

- C4 export covers Context, Container, and Component levels, each available as
  Structurizr DSL and C4 Mermaid, projected from the existing model.
- The UI lets a user drill Context → Container → Component for an application and
  see/export each level.
- All projections are pure and unit-tested against fixed inputs (no live data).

## Dependencies

Milestones 29 (C4 System-Context + `c4.rs` projection machinery), 08 (components,
use_case_components, dependency `component_id`, infrastructure relations), 10
(graph). Shares the C4 page/assets.

## Out of scope

The C4 **Code** level (class diagrams), deployment diagrams, and editing/authoring
diagrams — these stay derived-only projections of the model.
