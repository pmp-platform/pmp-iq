-- migrate:up
CREATE TABLE ai_agent_profiles (
    id            BLOB PRIMARY KEY,
    name          TEXT NOT NULL,
    provider_type TEXT NOT NULL,
    config        TEXT NOT NULL DEFAULT '{}',
    secrets_enc   BLOB,
    enabled       INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- migrate:down
