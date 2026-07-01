-- migrate:up
-- Roles, teams and team→application ownership (M37). A role row overrides the
-- default (viewer); the first admin is bootstrapped in code. Teams optionally
-- carry a tenant id for feature-flagged multi-tenant isolation.
CREATE TABLE roles (
    principal TEXT PRIMARY KEY,    -- username / oauth login
    role      TEXT NOT NULL        -- admin | maintainer | viewer
);

CREATE TABLE teams (
    id        UUID PRIMARY KEY,
    name      TEXT NOT NULL UNIQUE,
    tenant_id TEXT
);

CREATE TABLE team_members (
    team_id   UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    principal TEXT NOT NULL,
    PRIMARY KEY (team_id, principal)
);

CREATE TABLE team_applications (
    team_id        UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    PRIMARY KEY (team_id, application_id)
);

-- migrate:down
