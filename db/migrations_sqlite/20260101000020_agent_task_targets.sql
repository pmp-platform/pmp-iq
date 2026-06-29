-- migrate:up
-- Multi-repo agent task targets (see the Postgres migration for the rationale).
CREATE TABLE agent_task_targets (
    id            BLOB PRIMARY KEY,
    task_id       BLOB NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    repository_id BLOB NOT NULL,
    branch_name   TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    pr_url        TEXT,
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (task_id, repository_id)
);
CREATE INDEX idx_agent_task_targets_task ON agent_task_targets(task_id);

INSERT INTO agent_task_targets (id, task_id, repository_id, branch_name, status, pr_url)
SELECT id, id, repository_id, branch_name, status, pr_url FROM agent_tasks;

-- migrate:down
