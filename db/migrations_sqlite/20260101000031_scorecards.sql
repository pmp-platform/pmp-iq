-- migrate:up
-- Scorecards (M43) — see the Postgres migration for rationale.
CREATE TABLE scorecard_checks (
    id          TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    rule        TEXT NOT NULL,
    params      TEXT NOT NULL DEFAULT '{}',
    weight      INTEGER NOT NULL DEFAULT 1,
    severity    TEXT NOT NULL DEFAULT 'warn',
    enabled     INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE scorecard_results (
    id             BLOB PRIMARY KEY,
    application_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    check_id       TEXT NOT NULL,
    passed         INTEGER NOT NULL,
    detail         TEXT,
    evaluated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_scorecard_results_app ON scorecard_results(application_id, evaluated_at DESC);

-- migrate:down
