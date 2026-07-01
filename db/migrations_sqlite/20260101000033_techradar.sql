-- migrate:up
-- Version currency policy + tech radar (M45) — see the Postgres migration.
CREATE TABLE version_policy (
    id        BLOB PRIMARY KEY,
    ecosystem TEXT NOT NULL DEFAULT '',
    name      TEXT NOT NULL,
    latest    TEXT,
    eol_date  TEXT,
    UNIQUE (ecosystem, name)
);
CREATE TABLE tech_radar (
    id       BLOB PRIMARY KEY,
    quadrant TEXT NOT NULL,
    name     TEXT NOT NULL,
    ring     TEXT NOT NULL,
    note     TEXT,
    UNIQUE (quadrant, name)
);

-- migrate:down
