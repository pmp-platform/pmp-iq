# Milestone 16 — LLM hints for application entities

## Goal

Let users record free-text **hints** that correct or augment what the LLM
inferred for an application or any of its entities. Each entity (use case,
diagram, component, dependency, library, …) gets an **"LLM Hints"** button that
opens a modal with a textarea. Saved hints are injected into the analysis prompt
for that entity type / specific entity on the next sync, so the model can fix
mistakes or add the requested detail.

## Scope

- Persisted hints keyed to an application + entity type + (optional) specific
  entity, surviving the per-sync delete-and-recreate of sub-entities.
- CRUD endpoints + a reusable "LLM Hints" modal across all application entity
  types.
- Injection of an application's hints into the analyzer prompt on re-sync.

## Deliverables

### Data model

```sql
-- migrate:up
CREATE TABLE entity_hints (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    entity_type    TEXT NOT NULL,            -- 'application'|'dependency'|'library'|'use_case'
                                             -- |'component'|'diagram'|'observability_signal'
                                             -- |'language'|'infrastructure'|...
    entity_key     TEXT NOT NULL DEFAULT '', -- natural key (the entity's name); '' = whole type
    hint           TEXT NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (application_id, entity_type, entity_key)
);
-- migrate:down
```

- Hints are keyed by the entity's **natural key** (its `name`, or
  `name+ecosystem` for libraries, `target_name` for dependencies) — never the
  regenerated UUID, which changes on each sync. `entity_key = ''` is a
  type-level hint that applies to every entity of that type for the application.
- Cascade only on **application** delete, so hints survive the sub-entity
  delete-and-recreate that each sync performs.

### Repository + service

- `EntityHintRepository` trait (dual-engine Pg/SQLite impls + mock):
  `upsert`, `delete`, `get(app_id, entity_type, entity_key)`,
  `list_for_application(app_id)`.
- A service/loader that returns an application's hints as a structured map
  (`entity_type → { entity_key → hint }`) for prompt injection.

### Prompt injection

- Extend `AnalysisInput` (`src/platform/analyzer.rs`) with the application's
  hints (the struct already bundles parameters).
- `build_system_prompt` / `build_prompt` appends a **"User hints — authoritative
  corrections to honor"** section, grouped by entity type and key, reusing the
  existing section-builder pattern (`describe_kind` / `describe_property` style
  helpers). The model is told to treat hints as corrections that override its own
  inference.
- The sync job (`review/job.rs::analyze`) loads hints for the application that the
  repository maps to and passes them in. The first sync of a new application has
  no hints; hints take effect from the next sync onward.

### Routes + UI

- `GET` / `PUT` / `DELETE /platform/applications/{id}/hints` (under
  `require_auth`), addressing an entity via `entity_type` + `entity_key` query
  params (empty key = type-level).
- One reusable "LLM Hints" modal component (DRY) wired onto every application
  entity in the app-detail tabs — use cases, diagrams, components, dependencies,
  libraries, languages, infrastructure/linked entities, and the application
  itself. The modal prefills the saved hint and offers Save / Clear.
- Show an indicator (e.g., a badge on the button) when a hint already exists.

### Coverage

Apply the hints button to **all** application-related entity types, not just use
cases and diagrams (dependencies, libraries, components, observability signals,
languages, infrastructure, and the application).

## Tasks

- [ ] `entity_hints` migration (natural-key, application-cascade only).
- [ ] `EntityHintRepository` (Pg + SQLite) + mock; hint-loader service.
- [ ] Inject hints into the analyzer prompt; load them in the sync job.
- [ ] Hints CRUD routes.
- [ ] One reusable "LLM Hints" modal wired across all app entity types; existing-
      hint indicator.
- [ ] Unit tests: repo upsert/get/list (mocked store); prompt includes hints
      grouped by type/key; sync passes the right application's hints.

## Acceptance criteria

- A user can open "LLM Hints" on any application entity, save a hint, and see it
  persisted.
- After re-syncing, the inferred result for that entity reflects the hint.
- Hints survive across syncs (not wiped by sub-entity recreation) and are removed
  only when the application is deleted.
- Repository and prompt-injection logic are unit-tested with mocked dependencies.

## Dependencies

Milestones 08 (analysis + sub-entities), 09 (application detail page).

## Out of scope

Use-case/component file attribution and the File Explorer (M17).
