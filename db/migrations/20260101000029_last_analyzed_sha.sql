-- migrate:up
-- The commit last successfully analyzed for a repository (M41), used to diff
-- against the new HEAD and re-analyze only the affected entities.
ALTER TABLE repositories ADD COLUMN last_analyzed_sha TEXT;

-- migrate:down
