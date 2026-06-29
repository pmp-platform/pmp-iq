-- migrate:up
-- AI Agent change tasks (see the Postgres migration for the rationale).
CREATE TABLE agent_tasks (
    id             BLOB PRIMARY KEY,
    application_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    repository_id  BLOB NOT NULL,
    title          TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'draft',
    branch_name    TEXT NOT NULL,
    pr_url         TEXT,
    created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_agent_tasks_application ON agent_tasks(application_id, created_at DESC);

CREATE TABLE agent_task_messages (
    id           BLOB PRIMARY KEY,
    task_id      BLOB NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    role         TEXT NOT NULL,
    content      TEXT NOT NULL,
    execution_id BLOB,
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_agent_task_messages_task ON agent_task_messages(task_id, created_at);

-- migrate:down
