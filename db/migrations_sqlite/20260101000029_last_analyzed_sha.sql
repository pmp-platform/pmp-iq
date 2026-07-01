-- migrate:up
-- Last-analyzed commit per repository (M41) — see the Postgres migration.
ALTER TABLE repositories ADD COLUMN last_analyzed_sha TEXT;

-- migrate:down
