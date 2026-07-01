-- migrate:up
-- Gamification (M44) — see the Postgres migration for rationale.
CREATE TABLE xp_awards (
    id         BLOB PRIMARY KEY,
    actor      TEXT NOT NULL,
    reason     TEXT NOT NULL,
    points     INTEGER NOT NULL,
    skill      TEXT,
    source     TEXT,
    awarded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (actor, reason, source)
);
CREATE INDEX idx_xp_awards_actor ON xp_awards(actor);
CREATE TABLE badges (
    actor      TEXT NOT NULL,
    badge      TEXT NOT NULL,
    awarded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (actor, badge)
);

-- migrate:down
