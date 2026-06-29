-- migrate:up
-- Time-based scheduling: the controller polls jobs whose next_run_at elapsed.
ALTER TABLE jobs ADD COLUMN next_run_at TIMESTAMPTZ;
CREATE INDEX idx_jobs_next_run ON jobs(enabled, next_run_at);

-- Per-execution input and live-updatable structured metadata.
ALTER TABLE job_executions ADD COLUMN params   JSONB NOT NULL DEFAULT '{}';
ALTER TABLE job_executions ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}';

-- migrate:down
