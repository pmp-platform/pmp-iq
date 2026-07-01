-- migrate:up
-- Vector embeddings for catalog entities (M40), enabling semantic search,
-- similarity and duplicate detection. The vector is stored as a little-endian
-- f32 blob; nearest-neighbour search is a bounded cosine scan in Rust (the
-- catalog is small). `summary_hash` lets generation skip unchanged entities.
CREATE TABLE entity_embeddings (
    entity_type  TEXT NOT NULL,        -- application | component | use_case | library
    entity_id    UUID NOT NULL,
    model        TEXT NOT NULL,
    dim          INT  NOT NULL,
    vector       BYTEA NOT NULL,
    summary_hash TEXT NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (entity_type, entity_id, model)
);

-- migrate:down
