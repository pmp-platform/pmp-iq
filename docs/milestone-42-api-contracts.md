# Milestone 42 — API contracts & endpoint-level dependencies

## Goal

Extract each application's **exposed API surface** (REST/HTTP, gRPC and GraphQL
operations) and resolve the platform's outbound **dependencies down to the
specific endpoint** they call, not just "app → service". Today a dependency is a
free-form `target_name` + `kind` (M08) canonicalised to a catalog entity (M26);
this milestone turns the connection graph into a real **consumer ↔ producer
contract map**, enabling precise impact analysis ("who breaks if I change
`POST /charge`?") and contract-drift detection across syncs.

## Scope

- An `api_endpoints` model per application (operation, method/path or service/
  method, protocol, the component that serves it, request/response summary).
- Endpoint extraction added as a configurable analyzer section (M34).
- Dependency → endpoint resolution: match an outbound dependency to a producer's
  endpoint by path/operation, recorded on `application_dependencies`.
- Graph/C4 enrichment + impact analysis + an API tab on the application detail.

## Deliverables

### Endpoint model

App-owned, rebuilt per sync like components/use_cases (delete-and-recreate via
CASCADE, M08), with an `endpoint_files` attribution table (M17) so incremental
analysis (M41) can re-extract only affected endpoints:

```sql
-- migrate:up
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
ALTER TABLE application_dependencies ADD COLUMN endpoint_id UUID REFERENCES api_endpoints(id) ON DELETE SET NULL;
-- migrate:down
```

`AnalysisResult` gains an `endpoints` array (operation, protocol, summary,
component name, files); dependencies gain an optional `endpoint` field (the
producer operation they call). The strict-vocabulary rules of M34 apply
(protocols are an allowed-kind set).

### Resolution

After the writer upserts endpoints, a resolver matches each outbound dependency
whose `target_name` canonicalised to a known application (M26 catalog) to that
application's endpoint by operation string (exact → normalised → fuzzy, reusing
the `catalog::resolve_dependencies` matching machinery), writing
`application_dependencies.endpoint_id`. Unmatched dependencies stay app-level.

### Graph, C4 & impact

- The connection graph (M10) and C4 Container/Component views (M38) optionally
  label edges with the operation and can **expand** an app into its endpoints.
- `impact(endpoint_id)` returns the transitive set of applications/components
  that depend on an endpoint, for a blast-radius view.
- An **API** tab on the application detail lists the app's endpoints (grouped by
  protocol, each with its implementing component's files via the M17 explorer)
  and, per endpoint, its **consumers**.

### Contract drift

The change feed (M36) emits `endpoint created/updated/removed` events; a removed
or signature-changed endpoint that still has consumers is surfaced as a
**breaking-change warning** on the timeline.

## Tasks

- [ ] `api_endpoints` + `endpoint_files` migrations (both engines) + dependency
      `endpoint_id` column; writer upsert + delete-and-recreate.
- [ ] Configurable `endpoints` analyzer section (M34) + `AnalysisResult.endpoints`
      parse/validate; protocol allowed-kinds.
- [ ] Endpoint resolver (dependency → producer endpoint) reusing the catalog
      matcher; record `endpoint_id`.
- [ ] Graph/C4 edge labels + endpoint expansion; `impact(endpoint)` read layer.
- [ ] API tab (endpoints + consumers) on the app detail; breaking-change events.
- [ ] Unit tests (pure resolver + parse): endpoints parse and validate; a
      dependency resolves to the right producer endpoint; fuzzy/no-match behave;
      impact returns the consumer set; a removed-with-consumers endpoint warns.

## Acceptance criteria

- Applications expose their HTTP/gRPC/GraphQL operations in the model; outbound
  dependencies resolve to a producer's endpoint where one matches.
- The graph/C4 views show endpoint-level edges and an endpoint expansion;
  `impact(endpoint)` returns the dependent applications.
- Removed/changed endpoints with live consumers raise a breaking-change warning
  on the timeline; resolution and impact are unit-tested on both engines.

## Dependencies

Milestones 08 (analyzer/writer + dependencies + `component_id`), 17 (file
attribution), 26 (catalog canonicalisation), 10/38 (graph/C4), 34 (configurable
sections), 36 (change feed), 41 (incremental re-extraction of affected endpoints).

## Out of scope

Live contract testing/validation against running services, generating client SDKs
or server stubs, and full OpenAPI/proto round-tripping — this is a derived,
analysis-time contract map, not a runtime API gateway.
