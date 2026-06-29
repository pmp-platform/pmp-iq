-- migrate:up
-- A multi-repo agent task targets one or more repositories; each target carries
-- its own branch, PR, and status. The parent agent_tasks row keeps the overall
-- title/status (and, for single-repo tasks, the primary branch/PR).
CREATE TABLE agent_task_targets (
    id            UUID PRIMARY KEY,
    task_id       UUID NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    repository_id UUID NOT NULL,
    branch_name   TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    pr_url        TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (task_id, repository_id)
);
CREATE INDEX idx_agent_task_targets_task ON agent_task_targets(task_id);

-- Backfill one target per existing single-repo task.
INSERT INTO agent_task_targets (id, task_id, repository_id, branch_name, status, pr_url)
SELECT id, id, repository_id, branch_name, status, pr_url FROM agent_tasks;

-- migrate:down
