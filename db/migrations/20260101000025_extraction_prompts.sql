-- migrate:up
-- Editable per-section extraction prompt templates (M34). Defaults live in code;
-- a row here overrides the shipped default for that section (delete = reset).
CREATE TABLE extraction_prompts (
    section_key TEXT PRIMARY KEY,
    template    TEXT NOT NULL,
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- migrate:down
