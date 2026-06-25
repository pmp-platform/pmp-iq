-- migrate:up
CREATE TABLE jobs (
    id           UUID PRIMARY KEY,
    job_type     TEXT NOT NULL,
    name         TEXT NOT NULL,
    trigger_type TEXT NOT NULL,            -- 'manual' | 'cron'
    cron_expr    TEXT,
    config       JSONB NOT NULL DEFAULT '{}',
    enabled      BOOLEAN NOT NULL DEFAULT true,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE job_executions (
    id          UUID PRIMARY KEY,
    job_id      UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    status      TEXT NOT NULL,             -- queued|running|succeeded|failed|cancelled
    trigger     TEXT NOT NULL,             -- 'manual' | 'cron'
    started_at  TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    summary     JSONB,
    error       TEXT,
    logs        TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_job_executions_job ON job_executions(job_id, created_at DESC);

-- migrate:down
