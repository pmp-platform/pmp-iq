-- migrate:up
CREATE TABLE applications (
    id               BLOB PRIMARY KEY,
    repository_id    BLOB REFERENCES repositories(id) ON DELETE SET NULL,
    name             TEXT NOT NULL,
    app_type         TEXT,
    description      TEXT,
    primary_language TEXT,
    metadata         TEXT NOT NULL DEFAULT '{}',
    last_analyzed_at TEXT,
    created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (repository_id)
);

CREATE TABLE languages (id BLOB PRIMARY KEY, name TEXT UNIQUE NOT NULL);
CREATE TABLE application_languages (
    application_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    language_id    BLOB NOT NULL REFERENCES languages(id) ON DELETE CASCADE,
    percentage     REAL,
    PRIMARY KEY (application_id, language_id)
);

CREATE TABLE libraries (
    id BLOB PRIMARY KEY, name TEXT NOT NULL, ecosystem TEXT NOT NULL,
    UNIQUE (name, ecosystem)
);
CREATE TABLE library_versions (
    id BLOB PRIMARY KEY,
    library_id BLOB NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    version TEXT NOT NULL,
    UNIQUE (library_id, version)
);
CREATE TABLE application_libraries (
    application_id     BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    library_version_id BLOB NOT NULL REFERENCES library_versions(id) ON DELETE CASCADE,
    scope              TEXT,
    PRIMARY KEY (application_id, library_version_id)
);

CREATE TABLE infrastructure (
    id BLOB PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL, version TEXT,
    UNIQUE (name, kind, version)
);
CREATE TABLE application_infrastructure (
    application_id    BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    infrastructure_id BLOB NOT NULL REFERENCES infrastructure(id) ON DELETE CASCADE,
    usage             TEXT,
    PRIMARY KEY (application_id, infrastructure_id)
);

CREATE TABLE application_dependencies (
    id            BLOB PRIMARY KEY,
    source_app_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    target_app_id BLOB REFERENCES applications(id) ON DELETE SET NULL,
    target_name   TEXT NOT NULL,
    kind          TEXT,
    description   TEXT,
    UNIQUE (source_app_id, target_name, kind)
);

CREATE TABLE users (id BLOB PRIMARY KEY, username TEXT UNIQUE NOT NULL, email TEXT);
CREATE TABLE groups (id BLOB PRIMARY KEY, name TEXT UNIQUE NOT NULL);
CREATE TABLE group_memberships (
    group_id BLOB NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id  BLOB NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);
CREATE TABLE access_grants (
    id             BLOB PRIMARY KEY,
    application_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    principal_type TEXT NOT NULL,
    principal_id   BLOB NOT NULL,
    access_level   TEXT NOT NULL,
    UNIQUE (application_id, principal_type, principal_id, access_level)
);

-- migrate:down
