-- migrate:up
CREATE TABLE ai_agent_profiles (
    id            UUID PRIMARY KEY,
    name          TEXT NOT NULL,
    provider_type TEXT NOT NULL,        -- 'anthropic' | 'claude_cli'
    config        JSONB NOT NULL DEFAULT '{}',
    secrets_enc   BYTEA,                -- encrypted API key when applicable
    enabled       BOOLEAN NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- migrate:down
