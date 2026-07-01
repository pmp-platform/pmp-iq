-- migrate:up
-- Auto-remediation (M46) — see the Postgres migration for rationale.
CREATE TABLE remediation_rules (
    id           BLOB PRIMARY KEY,
    name         TEXT NOT NULL,
    trigger_kind TEXT NOT NULL,
    params       TEXT NOT NULL DEFAULT '{}',
    action       TEXT NOT NULL,
    prompt       TEXT NOT NULL,
    scope        TEXT NOT NULL DEFAULT '{}',
    auto_approve INTEGER NOT NULL DEFAULT 0,
    enabled      INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE remediations (
    id             BLOB PRIMARY KEY,
    rule_id        BLOB REFERENCES remediation_rules(id) ON DELETE SET NULL,
    application_id BLOB REFERENCES applications(id) ON DELETE CASCADE,
    finding_key    TEXT NOT NULL,
    status         TEXT NOT NULL,
    agent_task_id  BLOB,
    campaign_id    BLOB,
    created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (rule_id, finding_key)
);
CREATE INDEX idx_remediations_status ON remediations(status, created_at DESC);

-- migrate:down
