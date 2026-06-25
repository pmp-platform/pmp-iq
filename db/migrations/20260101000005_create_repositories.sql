-- migrate:up
CREATE TABLE repositories (
    id               UUID PRIMARY KEY,
    account_id       UUID NOT NULL REFERENCES repository_accounts(id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    full_name        TEXT NOT NULL,
    clone_url        TEXT NOT NULL,
    default_branch   TEXT,
    local_path       TEXT,
    last_cloned_at   TIMESTAMPTZ,
    last_commit_sha  TEXT,
    last_reviewed_at TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, full_name)
);

-- migrate:down
