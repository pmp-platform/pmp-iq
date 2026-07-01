-- migrate:up
ALTER TABLE repository_accounts ADD COLUMN organization TEXT;

-- migrate:down
