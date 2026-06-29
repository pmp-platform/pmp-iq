-- migrate:up
-- Per-application quality metrics (M31): tests, coverage, complexity, LOC, etc.
-- Flexible key/value with a source and timestamp, so new metrics are additive and
-- history enables trends.
CREATE TABLE application_metrics (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    metric_key     TEXT NOT NULL,
    value          DOUBLE PRECISION NOT NULL,
    unit           TEXT,
    source         TEXT NOT NULL,
    collected_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_application_metrics_app
    ON application_metrics(application_id, metric_key, collected_at DESC);

-- migrate:down
