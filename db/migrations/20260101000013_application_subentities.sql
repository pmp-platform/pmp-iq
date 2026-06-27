-- migrate:up
-- Configurable kinds gain a free-form config object (used by diagram /
-- observability-signal kinds).
ALTER TABLE entity_kinds ADD COLUMN config JSONB NOT NULL DEFAULT '{}';

-- Application-owned sub-entities. All are removed (CASCADE) when their owner is
-- deleted, so a re-sync wipe is one DELETE per top-level table.
CREATE TABLE components (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    kind           TEXT NOT NULL,
    description    TEXT,
    metadata       JSONB NOT NULL DEFAULT '{}',
    UNIQUE (application_id, name)
);

CREATE TABLE use_cases (
    id             UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    description    TEXT,
    metadata       JSONB NOT NULL DEFAULT '{}',
    UNIQUE (application_id, name)
);

CREATE TABLE use_case_components (
    use_case_id  UUID NOT NULL REFERENCES use_cases(id) ON DELETE CASCADE,
    component_id UUID NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    PRIMARY KEY (use_case_id, component_id)
);

CREATE TABLE diagrams (
    id          UUID PRIMARY KEY,
    use_case_id UUID NOT NULL REFERENCES use_cases(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,
    description TEXT,
    content     TEXT NOT NULL DEFAULT '',
    metadata    JSONB NOT NULL DEFAULT '{}',
    UNIQUE (use_case_id, name)
);

CREATE TABLE observability_signals (
    id           UUID PRIMARY KEY,
    component_id UUID NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    kind         TEXT NOT NULL,
    description  TEXT,
    metadata     JSONB NOT NULL DEFAULT '{}',
    UNIQUE (component_id, name)
);

-- Seed kinds for the new entity types (every list includes 'other').
INSERT INTO entity_kinds (id, entity_type, kind_id, name, description) VALUES
  (gen_random_uuid(),'components','controller','Controller','Handles inbound requests/input'),
  (gen_random_uuid(),'components','model','Model','Domain or data model'),
  (gen_random_uuid(),'components','repository','Repository','Persistence/data access'),
  (gen_random_uuid(),'components','service','Service','Business logic'),
  (gen_random_uuid(),'components','handler','Handler','Event/message handler'),
  (gen_random_uuid(),'components','middleware','Middleware','Cross-cutting request processing'),
  (gen_random_uuid(),'components','view','View','Presentation/UI'),
  (gen_random_uuid(),'components','other','Other','Anything else'),
  (gen_random_uuid(),'diagrams','flowchart','Flowchart','mermaid flowchart (graph)'),
  (gen_random_uuid(),'diagrams','sequence','Sequence','mermaid sequenceDiagram'),
  (gen_random_uuid(),'diagrams','class','Class','mermaid classDiagram'),
  (gen_random_uuid(),'diagrams','state','State','mermaid stateDiagram'),
  (gen_random_uuid(),'diagrams','er','Entity Relationship','mermaid erDiagram'),
  (gen_random_uuid(),'diagrams','component','Component','mermaid component/architecture diagram'),
  (gen_random_uuid(),'diagrams','other','Other','Any other mermaid diagram'),
  (gen_random_uuid(),'observability-signals','metric','Metric','Numeric measurement'),
  (gen_random_uuid(),'observability-signals','trace','Trace','Distributed trace span'),
  (gen_random_uuid(),'observability-signals','log','Log','Log event'),
  (gen_random_uuid(),'observability-signals','event','Event','Domain/business event'),
  (gen_random_uuid(),'observability-signals','other','Other','Anything else');

-- migrate:down
