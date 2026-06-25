-- migrate:up
CREATE TABLE repository_accounts (
    id              UUID PRIMARY KEY,
    name            TEXT NOT NULL,
    provider_type   TEXT NOT NULL,            -- 'github' | 'gitlab' | 'local'
    auth_type       TEXT NOT NULL,            -- 'token' | 'app' | 'none'
    base_url        TEXT,
    credentials_enc BYTEA,
    selection_mode  TEXT NOT NULL,            -- 'all' | 'regex' | 'list'
    selection_value TEXT,
    enabled         BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_repository_accounts_enabled ON repository_accounts(enabled);

-- migrate:down
