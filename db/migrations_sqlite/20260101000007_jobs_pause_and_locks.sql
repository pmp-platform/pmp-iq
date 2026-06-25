-- migrate:up
ALTER TABLE job_executions ADD COLUMN state TEXT;
ALTER TABLE job_executions ADD COLUMN resume_at TEXT;
ALTER TABLE job_executions ADD COLUMN pause_requested INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_job_executions_resume ON job_executions(status, resume_at);

CREATE TABLE controller_locks (
    name       TEXT PRIMARY KEY,
    holder     TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

-- migrate:down
