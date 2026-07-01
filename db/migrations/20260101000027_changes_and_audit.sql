-- migrate:up
-- Append-only change feed of platform-model mutations (M36), emitted by the
-- writer when a sync creates/updates/removes an entity keyed by its natural key.
CREATE TABLE platform_changes (
    id               UUID PRIMARY KEY,
    application_id   UUID REFERENCES applications(id) ON DELETE CASCADE,
    entity_type      TEXT NOT NULL,   -- application | dependency | library | member | metric | ...
    entity_key       TEXT NOT NULL,   -- natural key, stable across syncs
    change           TEXT NOT NULL,   -- created | updated | removed
    detail           JSONB,
    job_execution_id UUID,
    occurred_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_platform_changes_app ON platform_changes(application_id, occurred_at DESC);
CREATE INDEX idx_platform_changes_time ON platform_changes(occurred_at DESC);

-- Append-only audit log of operator actions (M36).
CREATE TABLE audit_events (
    id          UUID PRIMARY KEY,
    actor       TEXT NOT NULL,        -- principal username / "system"
    action      TEXT NOT NULL,        -- login | settings.update | job.run | agent_task.create | ...
    target      TEXT,
    metadata    JSONB,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_audit_events_time ON audit_events(occurred_at DESC);

-- migrate:down
