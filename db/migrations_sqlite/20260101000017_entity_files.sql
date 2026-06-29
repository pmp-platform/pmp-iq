-- migrate:up
-- Repository-relative files each component / use case affects (recreated per
-- sync alongside their owner via CASCADE).
CREATE TABLE component_files (
    component_id BLOB NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    path         TEXT NOT NULL,
    PRIMARY KEY (component_id, path)
);

CREATE TABLE use_case_files (
    use_case_id BLOB NOT NULL REFERENCES use_cases(id) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    PRIMARY KEY (use_case_id, path)
);

-- migrate:down
