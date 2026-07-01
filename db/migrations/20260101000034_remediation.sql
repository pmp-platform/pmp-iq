-- migrate:up
-- Auto-remediation (M46): rules mapping a finding trigger to an agent-task /
-- campaign action, and emitted remediations (deduped per rule + application).
CREATE TABLE remediation_rules (
    id           UUID PRIMARY KEY,
    name         TEXT NOT NULL,
    trigger_kind TEXT NOT NULL,   -- metric_below | metric_above | scorecard_failed | dep_eol
    params       JSONB NOT NULL DEFAULT '{}',
    action       TEXT NOT NULL,   -- agent_task | campaign
    prompt       TEXT NOT NULL,
    scope        JSONB NOT NULL DEFAULT '{}',
    auto_approve BOOLEAN NOT NULL DEFAULT FALSE,
    enabled      BOOLEAN NOT NULL DEFAULT TRUE
);
CREATE TABLE remediations (
    id             UUID PRIMARY KEY,
    rule_id        UUID REFERENCES remediation_rules(id) ON DELETE SET NULL,
    application_id UUID REFERENCES applications(id) ON DELETE CASCADE,
    finding_key    TEXT NOT NULL,
    status         TEXT NOT NULL,   -- proposed | approved | running | done | dismissed
    agent_task_id  UUID,
    campaign_id    UUID,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (rule_id, finding_key)
);
CREATE INDEX idx_remediations_status ON remediations(status, created_at DESC);

-- migrate:down
