-- migrate:up
CREATE TABLE applications (
    id               UUID PRIMARY KEY,
    repository_id    UUID REFERENCES repositories(id) ON DELETE SET NULL,
    name             TEXT NOT NULL,
    app_type         TEXT,
    description      TEXT,
    primary_language TEXT,
    metadata         JSONB NOT NULL DEFAULT '{}',
    last_analyzed_at TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (repository_id)
);

CREATE TABLE languages (
    id   UUID PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
);
CREATE TABLE application_languages (
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    language_id    UUID NOT NULL REFERENCES languages(id) ON DELETE CASCADE,
    percentage     NUMERIC,
    PRIMARY KEY (application_id, language_id)
);

CREATE TABLE libraries (
    id        UUID PRIMARY KEY,
    name      TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    UNIQUE (name, ecosystem)
);
CREATE TABLE library_versions (
    id         UUID PRIMARY KEY,
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    version    TEXT NOT NULL,
    UNIQUE (library_id, version)
);
CREATE TABLE application_libraries (
    application_id     UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    library_version_id UUID NOT NULL REFERENCES library_versions(id) ON DELETE CASCADE,
    scope              TEXT,
    PRIMARY KEY (application_id, library_version_id)
);

CREATE TABLE infrastructure (
    id      UUID PRIMARY KEY,
    name    TEXT NOT NULL,
    kind    TEXT NOT NULL,
    version TEXT,
    UNIQUE (name, kind, version)
);
CREATE TABLE application_infrastructure (
    application_id    UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    infrastructure_id UUID NOT NULL REFERENCES infrastructure(id) ON DELETE CASCADE,
    usage             TEXT,
    PRIMARY KEY (application_id, infrastructure_id)
);

CREATE TABLE application_dependencies (
    id            UUID PRIMARY KEY,
    source_app_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    target_app_id UUID REFERENCES applications(id) ON DELETE SET NULL,
    target_name   TEXT NOT NULL,
    kind          TEXT,
    description   TEXT,
    UNIQUE (source_app_id, target_name, kind)
);

CREATE TABLE users (
    id          UUID PRIMARY KEY,
    username    TEXT UNIQUE NOT NULL,
    email       TEXT
);
CREATE TABLE groups (
    id   UUID PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
);
CREATE TABLE group_memberships (
    group_id UUID NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);
CREATE TABLE access_grants (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    principal_type TEXT NOT NULL,
    principal_id   UUID NOT NULL,
    access_level   TEXT NOT NULL,
    UNIQUE (application_id, principal_type, principal_id, access_level)
);

-- migrate:down
