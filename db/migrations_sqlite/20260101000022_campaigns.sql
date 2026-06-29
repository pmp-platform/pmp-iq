-- migrate:up
-- Batch-change campaigns (see the Postgres migration for the rationale).
CREATE TABLE campaigns (
    id          BLOB PRIMARY KEY,
    name        TEXT NOT NULL,
    instruction TEXT NOT NULL,
    selection   TEXT NOT NULL,
    task_id     BLOB NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    status      TEXT NOT NULL DEFAULT 'running',
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_campaigns_created ON campaigns(created_at DESC);

-- migrate:down
