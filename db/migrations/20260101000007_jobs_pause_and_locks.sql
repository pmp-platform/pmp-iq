-- migrate:up
ALTER TABLE job_executions ADD COLUMN state JSONB;
ALTER TABLE job_executions ADD COLUMN resume_at TIMESTAMPTZ;
ALTER TABLE job_executions ADD COLUMN pause_requested BOOLEAN NOT NULL DEFAULT false;
CREATE INDEX idx_job_executions_resume ON job_executions(status, resume_at);

CREATE TABLE controller_locks (
    name       TEXT PRIMARY KEY,
    holder     TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

-- migrate:down
