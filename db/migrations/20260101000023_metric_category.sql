-- migrate:up
-- Expanded metrics catalog (M33): group each metric into a category so the
-- Insights panel and dashboard can render/rank them by theme. Additive: existing
-- rows default to 'general'. The category is derived from the metric key by the
-- application's metric registry at write time.
ALTER TABLE application_metrics ADD COLUMN category TEXT NOT NULL DEFAULT 'general';
CREATE INDEX idx_application_metrics_category
    ON application_metrics(application_id, category);

-- migrate:down
