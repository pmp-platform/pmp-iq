-- migrate:up
CREATE TABLE jobs (
    id           BLOB PRIMARY KEY,
    job_type     TEXT NOT NULL,
    name         TEXT NOT NULL,
    trigger_type TEXT NOT NULL,
    cron_expr    TEXT,
    config       TEXT NOT NULL DEFAULT '{}',
    enabled      INTEGER NOT NULL DEFAULT 1,
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE job_executions (
    id          BLOB PRIMARY KEY,
    job_id      BLOB NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    status      TEXT NOT NULL,
    trigger     TEXT NOT NULL,
    started_at  TEXT,
    finished_at TEXT,
    summary     TEXT,
    error       TEXT,
    logs        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_job_executions_job ON job_executions(job_id, created_at);

-- migrate:down
