-- migrate:up
-- Production-readiness scorecards (M43): check definitions (seeded from code,
-- editable) and per-application check results with history for trends.
CREATE TABLE scorecard_checks (
    id          TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    rule        TEXT NOT NULL,
    params      JSONB NOT NULL DEFAULT '{}',
    weight      INT  NOT NULL DEFAULT 1,
    severity    TEXT NOT NULL DEFAULT 'warn',
    enabled     BOOLEAN NOT NULL DEFAULT TRUE
);
CREATE TABLE scorecard_results (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    check_id       TEXT NOT NULL,
    passed         BOOLEAN NOT NULL,
    detail         JSONB,
    evaluated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_scorecard_results_app ON scorecard_results(application_id, evaluated_at DESC);

-- migrate:down
