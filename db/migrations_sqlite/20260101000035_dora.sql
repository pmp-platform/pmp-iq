-- migrate:up
-- DORA metrics (M47) — see the Postgres migration for rationale.
CREATE TABLE deployments (
    id              BLOB PRIMARY KEY,
    application_id  BLOB REFERENCES applications(id) ON DELETE CASCADE,
    environment     TEXT NOT NULL DEFAULT 'production',
    sha             TEXT,
    succeeded       INTEGER NOT NULL DEFAULT 1,
    deployed_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    first_commit_at TEXT
);
CREATE INDEX idx_deployments_app ON deployments(application_id, deployed_at DESC);
CREATE TABLE incidents (
    id             BLOB PRIMARY KEY,
    application_id BLOB REFERENCES applications(id) ON DELETE CASCADE,
    caused_by      BLOB REFERENCES deployments(id) ON DELETE SET NULL,
    opened_at      TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    resolved_at    TEXT
);
CREATE INDEX idx_incidents_app ON incidents(application_id, opened_at DESC);

-- migrate:down
