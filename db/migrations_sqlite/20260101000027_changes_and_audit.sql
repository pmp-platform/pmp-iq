-- migrate:up
-- Change feed + audit log (M36) — see the Postgres migration for rationale.
CREATE TABLE platform_changes (
    id               BLOB PRIMARY KEY,
    application_id   BLOB REFERENCES applications(id) ON DELETE CASCADE,
    entity_type      TEXT NOT NULL,
    entity_key       TEXT NOT NULL,
    change           TEXT NOT NULL,
    detail           TEXT,
    job_execution_id BLOB,
    occurred_at      TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_platform_changes_app ON platform_changes(application_id, occurred_at DESC);
CREATE INDEX idx_platform_changes_time ON platform_changes(occurred_at DESC);

CREATE TABLE audit_events (
    id          BLOB PRIMARY KEY,
    actor       TEXT NOT NULL,
    action      TEXT NOT NULL,
    target      TEXT,
    metadata    TEXT,
    occurred_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_audit_events_time ON audit_events(occurred_at DESC);

-- migrate:down
