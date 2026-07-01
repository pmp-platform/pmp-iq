-- migrate:up
-- LLM usage rows (M39): one per recorded LLM call, so spend is queryable
-- without scanning execution metadata. Aggregated into cost via a configurable
-- per-model price map.
CREATE TABLE llm_usage (
    id               UUID PRIMARY KEY,
    job_execution_id UUID NOT NULL,
    application_id   UUID,
    ai_profile_id    UUID,
    model            TEXT NOT NULL,
    input_tokens     BIGINT NOT NULL,
    output_tokens    BIGINT NOT NULL,
    occurred_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_llm_usage_app ON llm_usage(application_id, occurred_at DESC);
CREATE INDEX idx_llm_usage_profile ON llm_usage(ai_profile_id, occurred_at DESC);
CREATE INDEX idx_llm_usage_exec ON llm_usage(job_execution_id);

-- Budgets (M39): warn/hard-stop spend limits per scope and period.
CREATE TABLE llm_budgets (
    id         UUID PRIMARY KEY,
    scope      TEXT NOT NULL,              -- global | profile | job | application
    scope_id   UUID,                       -- null for global
    period     TEXT NOT NULL,              -- daily | monthly
    limit_usd  DOUBLE PRECISION NOT NULL,
    hard_stop  BOOLEAN NOT NULL DEFAULT FALSE
);

-- migrate:down
