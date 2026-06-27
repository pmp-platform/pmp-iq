-- migrate:up
-- Configurable kinds gain a free-form config object (used by diagram /
-- observability-signal kinds).
ALTER TABLE entity_kinds ADD COLUMN config TEXT NOT NULL DEFAULT '{}';

-- Application-owned sub-entities. All are removed (CASCADE) when their owner is
-- deleted, so a re-sync wipe is one DELETE per top-level table.
CREATE TABLE components (
    id             BLOB PRIMARY KEY,
    application_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    kind           TEXT NOT NULL,
    description    TEXT,
    metadata       TEXT NOT NULL DEFAULT '{}',
    UNIQUE (application_id, name)
);

CREATE TABLE use_cases (
    id             BLOB PRIMARY KEY,
    application_id BLOB NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    description    TEXT,
    metadata       TEXT NOT NULL DEFAULT '{}',
    UNIQUE (application_id, name)
);

CREATE TABLE use_case_components (
    use_case_id  BLOB NOT NULL REFERENCES use_cases(id) ON DELETE CASCADE,
    component_id BLOB NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    PRIMARY KEY (use_case_id, component_id)
);

CREATE TABLE diagrams (
    id          BLOB PRIMARY KEY,
    use_case_id BLOB NOT NULL REFERENCES use_cases(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,
    description TEXT,
    content     TEXT NOT NULL DEFAULT '',
    metadata    TEXT NOT NULL DEFAULT '{}',
    UNIQUE (use_case_id, name)
);

CREATE TABLE observability_signals (
    id           BLOB PRIMARY KEY,
    component_id BLOB NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    kind         TEXT NOT NULL,
    description  TEXT,
    metadata     TEXT NOT NULL DEFAULT '{}',
    UNIQUE (component_id, name)
);

-- Seed kinds for the new entity types (every list includes 'other').
INSERT INTO entity_kinds (id, entity_type, kind_id, name, description) VALUES
  (randomblob(16),'components','controller','Controller','Handles inbound requests/input'),
  (randomblob(16),'components','model','Model','Domain or data model'),
  (randomblob(16),'components','repository','Repository','Persistence/data access'),
  (randomblob(16),'components','service','Service','Business logic'),
  (randomblob(16),'components','handler','Handler','Event/message handler'),
  (randomblob(16),'components','middleware','Middleware','Cross-cutting request processing'),
  (randomblob(16),'components','view','View','Presentation/UI'),
  (randomblob(16),'components','other','Other','Anything else'),
  (randomblob(16),'diagrams','flowchart','Flowchart','mermaid flowchart (graph)'),
  (randomblob(16),'diagrams','sequence','Sequence','mermaid sequenceDiagram'),
  (randomblob(16),'diagrams','class','Class','mermaid classDiagram'),
  (randomblob(16),'diagrams','state','State','mermaid stateDiagram'),
  (randomblob(16),'diagrams','er','Entity Relationship','mermaid erDiagram'),
  (randomblob(16),'diagrams','component','Component','mermaid component/architecture diagram'),
  (randomblob(16),'diagrams','other','Other','Any other mermaid diagram'),
  (randomblob(16),'observability-signals','metric','Metric','Numeric measurement'),
  (randomblob(16),'observability-signals','trace','Trace','Distributed trace span'),
  (randomblob(16),'observability-signals','log','Log','Log event'),
  (randomblob(16),'observability-signals','event','Event','Domain/business event'),
  (randomblob(16),'observability-signals','other','Other','Anything else');

-- migrate:down
