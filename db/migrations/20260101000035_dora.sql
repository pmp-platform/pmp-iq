-- migrate:up
-- DORA metrics (M47): captured deployment + incident events over which the four
-- DORA measures (deploy frequency, lead time, change-failure rate, MTTR) are
-- derived per application / team / fleet.
CREATE TABLE deployments (
    id              UUID PRIMARY KEY,
    application_id  UUID REFERENCES applications(id) ON DELETE CASCADE,
    environment     TEXT NOT NULL DEFAULT 'production',
    sha             TEXT,
    succeeded       BOOLEAN NOT NULL DEFAULT TRUE,
    deployed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- first commit of the change (for lead time), when resolvable.
    first_commit_at TIMESTAMPTZ
);
CREATE INDEX idx_deployments_app ON deployments(application_id, deployed_at DESC);
CREATE TABLE incidents (
    id             UUID PRIMARY KEY,
    application_id UUID REFERENCES applications(id) ON DELETE CASCADE,
    caused_by      UUID REFERENCES deployments(id) ON DELETE SET NULL,
    opened_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at    TIMESTAMPTZ
);
CREATE INDEX idx_incidents_app ON incidents(application_id, opened_at DESC);

-- migrate:down
