-- migrate:up
-- Expanded metrics catalog (M33) — see the Postgres migration for rationale.
ALTER TABLE application_metrics ADD COLUMN category TEXT NOT NULL DEFAULT 'general';
CREATE INDEX idx_application_metrics_category
    ON application_metrics(application_id, category);

-- migrate:down
