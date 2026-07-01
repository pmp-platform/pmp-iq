-- migrate:up
-- Operator gamification (M44): XP awards derived idempotently from recorded
-- actions (keyed by source so replays don't double-count) and earned badges.
CREATE TABLE xp_awards (
    id         UUID PRIMARY KEY,
    actor      TEXT NOT NULL,
    reason     TEXT NOT NULL,
    points     INT  NOT NULL,
    skill      TEXT,
    source     TEXT,
    awarded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (actor, reason, source)
);
CREATE INDEX idx_xp_awards_actor ON xp_awards(actor);
CREATE TABLE badges (
    actor      TEXT NOT NULL,
    badge      TEXT NOT NULL,
    awarded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (actor, badge)
);

-- migrate:down
