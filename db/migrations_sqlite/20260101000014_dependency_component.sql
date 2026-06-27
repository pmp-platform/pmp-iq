-- migrate:up
-- Dependencies are outbound connections detected from code, each mapped to the
-- component that makes the connection. Recreate the (transient, per-sync) table
-- with a nullable component link and a uniqueness that allows several components
-- to connect to the same target as distinct edges.
DROP TABLE IF EXISTS application_dependencies;
CREATE TABLE application_dependencies (
    id            BLOB PRIMARY KEY,
    source_app_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    component_id  BLOB REFERENCES components(id) ON DELETE SET NULL,
    target_app_id BLOB REFERENCES applications(id) ON DELETE SET NULL,
    target_name   TEXT NOT NULL,
    kind          TEXT,
    description   TEXT,
    UNIQUE (source_app_id, component_id, target_name, kind)
);

-- migrate:down
