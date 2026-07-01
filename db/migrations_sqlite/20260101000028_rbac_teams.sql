-- migrate:up
-- Roles, teams and ownership (M37) — see the Postgres migration for rationale.
CREATE TABLE roles (
    principal TEXT PRIMARY KEY,
    role      TEXT NOT NULL
);

CREATE TABLE teams (
    id        BLOB PRIMARY KEY,
    name      TEXT NOT NULL UNIQUE,
    tenant_id TEXT
);

CREATE TABLE team_members (
    team_id   BLOB NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    principal TEXT NOT NULL,
    PRIMARY KEY (team_id, principal)
);

CREATE TABLE team_applications (
    team_id        BLOB NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    application_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    PRIMARY KEY (team_id, application_id)
);

-- migrate:down
