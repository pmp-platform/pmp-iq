-- migrate:up
-- Entity embeddings (M40) — see the Postgres migration for rationale.
CREATE TABLE entity_embeddings (
    entity_type  TEXT NOT NULL,
    entity_id    BLOB NOT NULL,
    model        TEXT NOT NULL,
    dim          INTEGER NOT NULL,
    vector       BLOB NOT NULL,
    summary_hash TEXT NOT NULL,
    updated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (entity_type, entity_id, model)
);

-- migrate:down
