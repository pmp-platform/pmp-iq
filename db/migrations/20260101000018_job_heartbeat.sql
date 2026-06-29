-- migrate:up
-- Liveness heartbeat: a running job updates heartbeat_at; the controller cancels
-- executions that haven't beaten within the stale threshold.
ALTER TABLE job_executions ADD COLUMN heartbeat_at TIMESTAMPTZ;
CREATE INDEX idx_job_executions_heartbeat ON job_executions(status, heartbeat_at);

-- migrate:down
