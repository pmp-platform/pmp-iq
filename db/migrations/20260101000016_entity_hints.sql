-- migrate:up
-- Free-text hints that correct/augment what the LLM inferred for an application
-- or one of its entities. Keyed by the entity's natural key (its name), so they
-- survive the per-sync delete-and-recreate of sub-entities; only an application
-- delete cascades them away. entity_key = '' applies to the whole entity type.
CREATE TABLE entity_hints (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    entity_type    TEXT NOT NULL,
    entity_key     TEXT NOT NULL DEFAULT '',
    hint           TEXT NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (application_id, entity_type, entity_key)
);

-- migrate:down
