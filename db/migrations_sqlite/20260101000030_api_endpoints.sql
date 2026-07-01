-- migrate:up
-- API endpoints (M42) — see the Postgres migration for rationale.
CREATE TABLE api_endpoints (
    id             BLOB PRIMARY KEY,
    application_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    protocol       TEXT NOT NULL,
    operation      TEXT NOT NULL,
    summary        TEXT,
    component_id   BLOB REFERENCES components(id) ON DELETE SET NULL,
    metadata       TEXT NOT NULL DEFAULT '{}',
    UNIQUE (application_id, protocol, operation)
);
CREATE TABLE endpoint_files (
    endpoint_id BLOB NOT NULL REFERENCES api_endpoints(id) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    PRIMARY KEY (endpoint_id, path)
);
ALTER TABLE application_dependencies ADD COLUMN endpoint_id BLOB REFERENCES api_endpoints(id) ON DELETE SET NULL;

-- migrate:down
