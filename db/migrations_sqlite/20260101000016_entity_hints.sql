-- migrate:up
-- Free-text hints that correct/augment what the LLM inferred for an application
-- or one of its entities (see the Postgres migration for the rationale).
CREATE TABLE entity_hints (
    id             BLOB PRIMARY KEY,
    application_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    entity_type    TEXT NOT NULL,
    entity_key     TEXT NOT NULL DEFAULT '',
    hint           TEXT NOT NULL,
    updated_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (application_id, entity_type, entity_key)
);

-- migrate:down
