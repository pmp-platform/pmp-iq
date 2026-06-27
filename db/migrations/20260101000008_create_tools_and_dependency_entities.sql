-- migrate:up
CREATE TABLE tools (
    id       UUID PRIMARY KEY,
    name     TEXT NOT NULL,
    kind     TEXT NOT NULL,
    version  TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    UNIQUE (name, kind, version)
);
CREATE TABLE application_tools (
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    tool_id        UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    usage          TEXT,
    PRIMARY KEY (application_id, tool_id)
);

CREATE TABLE cloud_providers (
    id       UUID PRIMARY KEY,
    name     TEXT NOT NULL,
    kind     TEXT NOT NULL,
    version  TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    UNIQUE (name, kind, version)
);
CREATE TABLE application_cloud_providers (
    application_id    UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    cloud_provider_id UUID NOT NULL REFERENCES cloud_providers(id) ON DELETE CASCADE,
    usage             TEXT,
    PRIMARY KEY (application_id, cloud_provider_id)
);

CREATE TABLE services (
    id       UUID PRIMARY KEY,
    name     TEXT NOT NULL,
    kind     TEXT NOT NULL,
    version  TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    UNIQUE (name, kind, version)
);
CREATE TABLE application_services (
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    service_id     UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    usage          TEXT,
    PRIMARY KEY (application_id, service_id)
);

CREATE TABLE platforms (
    id       UUID PRIMARY KEY,
    name     TEXT NOT NULL,
    kind     TEXT NOT NULL,
    version  TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    UNIQUE (name, kind, version)
);
CREATE TABLE application_platforms (
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    platform_id    UUID NOT NULL REFERENCES platforms(id) ON DELETE CASCADE,
    usage          TEXT,
    PRIMARY KEY (application_id, platform_id)
);

CREATE TABLE external_deps (
    id       UUID PRIMARY KEY,
    name     TEXT NOT NULL,
    kind     TEXT NOT NULL,
    version  TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    UNIQUE (name, kind, version)
);
CREATE TABLE application_external_deps (
    application_id  UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    external_dep_id UUID NOT NULL REFERENCES external_deps(id) ON DELETE CASCADE,
    usage           TEXT,
    PRIMARY KEY (application_id, external_dep_id)
);

-- Bring the pre-existing infrastructure table into the linked-entity shape.
ALTER TABLE infrastructure ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}';

-- migrate:down
