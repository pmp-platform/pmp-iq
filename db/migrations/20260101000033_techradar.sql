-- migrate:up
-- Version currency policy + tech radar (M45). `ecosystem` is '' for
-- languages/runtimes. `eol_date` is an ISO date string (portable across engines).
CREATE TABLE version_policy (
    id        UUID PRIMARY KEY,
    ecosystem TEXT NOT NULL DEFAULT '',
    name      TEXT NOT NULL,
    latest    TEXT,
    eol_date  TEXT,
    UNIQUE (ecosystem, name)
);
CREATE TABLE tech_radar (
    id       UUID PRIMARY KEY,
    quadrant TEXT NOT NULL,   -- language | framework | infrastructure | tool
    name     TEXT NOT NULL,
    ring     TEXT NOT NULL,   -- adopt | trial | assess | hold
    note     TEXT,
    UNIQUE (quadrant, name)
);

-- migrate:down
