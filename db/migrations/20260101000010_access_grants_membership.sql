-- migrate:up
-- Rebuild access_grants as a single application <-> principal association that
-- carries the association type (member / ex_member / codeowner) and the raw
-- provider permissions. Members/ex-members come from the git provider API;
-- codeowners are AI-extracted from CODEOWNERS. One row per principal per app
-- (association_type / access_level / permissions are mutable attributes).
-- Pre-release: no data to preserve, so the table is recreated.
DROP TABLE IF EXISTS access_grants;
CREATE TABLE access_grants (
    id               UUID PRIMARY KEY,
    application_id   UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    principal_type   TEXT NOT NULL,
    principal_id     UUID NOT NULL,
    association_type TEXT NOT NULL DEFAULT 'codeowner',
    access_level     TEXT,
    permissions      JSONB NOT NULL DEFAULT '{}',
    UNIQUE (application_id, principal_type, principal_id)
);

-- migrate:down
