-- migrate:up
CREATE TABLE repository_accounts (
    id              BLOB PRIMARY KEY,
    name            TEXT NOT NULL,
    provider_type   TEXT NOT NULL,
    auth_type       TEXT NOT NULL,
    base_url        TEXT,
    credentials_enc BLOB,
    selection_mode  TEXT NOT NULL,
    selection_value TEXT,
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at      TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_repository_accounts_enabled ON repository_accounts(enabled);

-- migrate:down
