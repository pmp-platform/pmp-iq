-- migrate:up
-- Time-based scheduling: the controller polls jobs whose next_run_at elapsed.
ALTER TABLE jobs ADD COLUMN next_run_at TEXT;
CREATE INDEX idx_jobs_next_run ON jobs(enabled, next_run_at);

-- Per-execution input and live-updatable structured metadata.
ALTER TABLE job_executions ADD COLUMN params   TEXT NOT NULL DEFAULT '{}';
ALTER TABLE job_executions ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}';

-- migrate:down
