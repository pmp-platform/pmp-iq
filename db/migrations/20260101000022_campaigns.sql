-- migrate:up
-- Batch-change campaigns (M30): a named, declarative change applied across many
-- repositories. A campaign drives one multi-repo agent task (its per-repo
-- targets carry the branch/PR/status), so progress reuses agent_task_targets.
CREATE TABLE campaigns (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    instruction TEXT NOT NULL,
    selection   TEXT NOT NULL,
    task_id     UUID NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    status      TEXT NOT NULL DEFAULT 'running',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_campaigns_created ON campaigns(created_at DESC);

-- migrate:down
