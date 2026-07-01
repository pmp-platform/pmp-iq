-- migrate:up
-- Application API surface (M42): the HTTP/gRPC/GraphQL operations each app
-- exposes, plus the file attribution that lets incremental analysis re-extract
-- only affected endpoints. Outbound dependencies gain an optional link to the
-- producer endpoint they call.
CREATE TABLE api_endpoints (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    protocol       TEXT NOT NULL,   -- http | grpc | graphql
    operation      TEXT NOT NULL,   -- "POST /charge" | "billing.Charge" | "mutation pay"
    summary        TEXT,
    component_id   UUID REFERENCES components(id) ON DELETE SET NULL,
    metadata       JSONB NOT NULL DEFAULT '{}',
    UNIQUE (application_id, protocol, operation)
);
CREATE TABLE endpoint_files (
    endpoint_id UUID NOT NULL REFERENCES api_endpoints(id) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    PRIMARY KEY (endpoint_id, path)
);
ALTER TABLE application_dependencies
    ADD COLUMN endpoint_id UUID REFERENCES api_endpoints(id) ON DELETE SET NULL;

-- migrate:down
