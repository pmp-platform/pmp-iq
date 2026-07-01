-- migrate:up
-- Editable per-section extraction prompt templates (M34) — see the Postgres
-- migration for rationale.
CREATE TABLE extraction_prompts (
    section_key TEXT PRIMARY KEY,
    template    TEXT NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    updated_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- migrate:down
