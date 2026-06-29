-- migrate:up
-- Liveness heartbeat (see the Postgres migration for the rationale).
ALTER TABLE job_executions ADD COLUMN heartbeat_at TEXT;
CREATE INDEX idx_job_executions_heartbeat ON job_executions(status, heartbeat_at);

-- migrate:down
