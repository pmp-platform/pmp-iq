# Milestone 34 — Configurable extraction prompts (per section, in Settings)

## Goal

Make the prompt for **each extraction section** individually configurable in
Settings — applications, components, use cases, dependencies, members, diagrams,
observability signals, and **metrics** (M31/M33) each get their own editable
prompt part. Today there is a single `SYSTEM_PROMPT` const plus
`build_system_prompt(config)` that only injects the allowed kinds/properties; this
milestone externalises the prompt **text** per section and lets operators
edit/override it from the UI, with the built-in defaults as fallback, so tuning
what the LLM extracts needs no code change or redeploy.

## Scope

- A per-section prompt **template** store (defaults seeded; editable; reset to
  default).
- The analyzer composes its system/user prompt from the configured section blocks
  (base + per-section), preserving the **strict** kinds/properties injection and
  the required JSON-schema contract.
- A dedicated **metrics** prompt section (so M33's metric collection prompt is
  itself customisable).
- A Settings tab to edit each section with placeholder validation so a bad
  template cannot break extraction.

## Deliverables

### Prompt store

A dual-engine store, seeded with the current built-in defaults so behaviour is
unchanged until edited:

```sql
-- migrate:up
CREATE TABLE extraction_prompts (
    section_key TEXT PRIMARY KEY,     -- base | applications | components | use_cases |
                                      -- dependencies | members | diagrams |
                                      -- observability | metrics
    template    TEXT NOT NULL,
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- migrate:down
```

`PromptConfig` (loaded alongside `AnalysisConfig`) carries the section templates;
`AnalysisConfigService::load()` returns both.

### Prompt composition

`build_system_prompt` is generalised to compose from `PromptConfig`:

- `base` block, then each **enabled** section block in a fixed order.
- The **kinds/properties** injection (current strict behaviour) and the JSON
  output schema remain appended programmatically — they are **required**
  placeholders, validated on save.
- `AnalysisInput` already carries `config`; it gains the resolved `PromptConfig`
  (or `AnalysisConfig` absorbs it) so the analyzer/metrics job read the active
  templates.

### Validation & safety

Saving a section validates that required placeholders are present (e.g.
`{{json_schema}}`, `{{kinds}}`, `{{properties}}`, `{{hints}}` where applicable)
and that the template is non-empty; an invalid template is rejected with a clear
error and the previous/default template stays active. A **Reset to default**
restores the shipped text. Optionally record which template version a sync used in
the execution metadata for auditability (ties to M36).

### UI & routes

A Settings **"Extraction prompts"** tab listing each section with an editor,
enabled toggle, placeholder hints, and reset; routes extend
`routes/analysis_config.rs` (or a new `routes/prompts.rs`):
`GET/PUT /api/analysis-config/prompts/:section`, `POST .../reset`.

## Tasks

- [ ] `extraction_prompts` migration (both engines) + dual-engine repository,
      seeded with current defaults.
- [ ] `PromptConfig` + generalise `build_system_prompt` to compose enabled
      sections while keeping strict kinds/properties + schema injection.
- [ ] Placeholder/required-field validation on save; reset-to-default.
- [ ] Wire the metrics job (M31/M33) to the configurable metrics section.
- [ ] Settings "Extraction prompts" tab + routes.
- [ ] Unit tests (mocked repo): composed prompt contains base + enabled sections +
      injected kinds/properties/schema; a missing required placeholder is rejected;
      reset restores default; a disabled section is omitted.

## Acceptance criteria

- Each extraction section (including metrics) has an editable prompt in Settings;
  edits take effect on the next sync without code changes.
- The strict kind/property vocabulary and JSON output contract are always enforced
  regardless of edits; invalid templates are rejected, not silently applied.
- Defaults are seeded so behaviour is unchanged until an operator edits a section;
  composition is unit-tested with mocked storage.

## Dependencies

Milestones 08 (analyzer + `build_system_prompt`), the analysis-config store/
service & Settings tabs, 31/33 (metrics prompt). Auditing of used templates ties
to M36.

## Out of scope

A full template language with loops/conditionals (placeholders only),
per-application prompt overrides, and prompt A/B testing — a single configurable
default set per section.
