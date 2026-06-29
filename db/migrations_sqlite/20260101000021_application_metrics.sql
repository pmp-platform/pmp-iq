-- migrate:up
-- Per-application quality metrics (see the Postgres migration for rationale).
CREATE TABLE application_metrics (
    id             BLOB PRIMARY KEY,
    application_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    metric_key     TEXT NOT NULL,
    value          REAL NOT NULL,
    unit           TEXT,
    source         TEXT NOT NULL,
    collected_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_application_metrics_app
    ON application_metrics(application_id, metric_key, collected_at DESC);

-- migrate:down
