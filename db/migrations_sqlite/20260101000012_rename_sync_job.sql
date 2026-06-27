-- migrate:up
-- The review-repositories job is now "sync-repositories"; update existing rows so
-- they resolve against the renamed job-type registry.
UPDATE jobs SET job_type = 'sync-repositories' WHERE job_type = 'review-repositories';

-- migrate:down
