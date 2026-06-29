# Milestone 29 — C4 model views & export

## Goal

Project the platform model into the **C4 model** (Context → Container →
Component → Code) and render standard, zoomable C4 views, plus export to
**Structurizr DSL** and **C4 Mermaid/PlantUML**. Because PlatIQ's model is
auto-derived from code, the C4 views are always current — and can be diffed
against a committed architecture-as-code file to flag **drift**.

## Scope

- A projection mapping existing entities to C4 elements + relationships per level.
- Four navigable levels (Context, Container, Component, Code) reusing the catalog,
  the application sub-entities, and the codebase map.
- Export to Structurizr DSL and C4-Mermaid; optional drift check against a repo's
  committed C4/Structurizr file.

## Deliverables

### Model → C4 projection

A `c4` projection service mapping the platform model onto C4 with no new analysis
(it re-views data the catalog already holds):

- **System Context (L1)** — each application is a *software system*; `external`
  linked entities are *external systems*; users/groups are *people/actors*;
  application dependencies become *relationships* (with the `component` label
  the writer already resolves). One context view per system + a landscape view.
- **Container (L2)** — for one application, its *containers*: the app's runtime
  pieces plus the datastores/queues/services it depends on (the `infrastructure`,
  `services`, `cloud-providers`, `platforms` linked entities), with the
  technology tag from each entity's kind/metadata.
- **Component (L3)** — the application's `components` sub-entities (M08) and their
  intra-app relationships (reuse the component graph already shown on the
  Components tab / per-use-case component diagram).
- **Code (L4)** — links to the codebase map (M28) for the selected component's
  files (M17 attribution) — the lowest zoom level.

Each element carries a stable id, name, description, and technology tag so views
and exports are deterministic.

### Views (UI)

- A **"C4"** section/tab with the four zoom levels and breadcrumb navigation
  (Landscape → System → Container → Component → Code), rendered with the existing
  diagram stack (Mermaid C4 and/or G6), with the standard zoom/reset controls.
- Each level cross-links: a container opens its components; a component opens its
  code (M28); relationships show their description + technology.

### Export & drift

- **Export**: generate a single **Structurizr DSL** workspace (model + the four
  views) and **C4 Mermaid** diagrams per level (the analyzer already emits Mermaid
  for use cases — extend to the C4 shapes). Downloadable / copyable, and embeddable.
- **Drift (optional)**: if a repo commits an architecture-as-code file
  (`workspace.dsl` / a C4 file), compare the derived model against it and surface
  differences (missing/extra containers or relationships) — the "diagram vs
  reality" check, made possible because our model is ground-truth from code.

## Tasks

- [ ] `c4` projection: entities → C4 elements/relationships at L1–L3, linking L4
      to the codebase map.
- [ ] C4 views (Landscape/System/Container/Component/Code) with breadcrumb nav,
      reusing the Mermaid/G6 diagram stack.
- [ ] Export: Structurizr DSL workspace + C4 Mermaid per level.
- [ ] Optional drift check vs a committed Structurizr/C4 file.
- [ ] Unit tests: projection maps a sample model to the expected C4 elements per
      level; DSL/Mermaid export is deterministic for a fixed model; drift detects
      a missing/extra relationship.

## Acceptance criteria

- The platform is browsable as C4 at Context, Container, Component, and Code
  levels, each zoomable and cross-linked, derived from the existing model.
- The model exports to valid Structurizr DSL and C4 Mermaid.
- (If enabled) drift against a committed architecture-as-code file is reported.
- The projection and export are unit-tested against a fixed model.

## Dependencies

Milestones 08 (components + dependencies + Mermaid generation), 09/10 (catalog +
graph), 17 (file attribution), 28 (codebase map for the Code level).

## Out of scope

A C4 diagram **editor** (views are generated from the model, not hand-drawn),
authoring new architecture that isn't in the code, and non-C4 notations.
