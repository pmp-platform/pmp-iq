-- migrate:up
-- AI Agent change tasks: each task is a session with an agentic AI over an
-- application's repository. The agent edits files on a dedicated branch, commits,
-- pushes, and opens a pull request. Messages capture the multi-turn transcript.
CREATE TABLE agent_tasks (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    repository_id  UUID NOT NULL,
    title          TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'draft',
    branch_name    TEXT NOT NULL,
    pr_url         TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_agent_tasks_application ON agent_tasks(application_id, created_at DESC);

CREATE TABLE agent_task_messages (
    id           UUID PRIMARY KEY,
    task_id      UUID NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    role         TEXT NOT NULL,
    content      TEXT NOT NULL,
    execution_id UUID,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_agent_task_messages_task ON agent_task_messages(task_id, created_at);

-- migrate:down
