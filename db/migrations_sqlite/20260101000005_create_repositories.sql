-- migrate:up
CREATE TABLE repositories (
    id               BLOB PRIMARY KEY,
    account_id       BLOB NOT NULL REFERENCES repository_accounts(id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    full_name        TEXT NOT NULL,
    clone_url        TEXT NOT NULL,
    default_branch   TEXT,
    local_path       TEXT,
    last_cloned_at   TEXT,
    last_commit_sha  TEXT,
    last_reviewed_at TEXT,
    created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (account_id, full_name)
);

-- migrate:down
