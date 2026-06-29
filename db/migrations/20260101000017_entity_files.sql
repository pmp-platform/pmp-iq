-- migrate:up
-- Repository-relative files each component / use case affects. Recreated per
-- sync alongside their owner (CASCADE), matching the sub-entity lifecycle.
CREATE TABLE component_files (
    component_id UUID NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    path         TEXT NOT NULL,
    PRIMARY KEY (component_id, path)
);

CREATE TABLE use_case_files (
    use_case_id UUID NOT NULL REFERENCES use_cases(id) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    PRIMARY KEY (use_case_id, path)
);

-- migrate:down
