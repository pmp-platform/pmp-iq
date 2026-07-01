-- migrate:up
-- LLM usage + budgets (M39) — see the Postgres migration for rationale.
CREATE TABLE llm_usage (
    id               BLOB PRIMARY KEY,
    job_execution_id BLOB NOT NULL,
    application_id   BLOB,
    ai_profile_id    BLOB,
    model            TEXT NOT NULL,
    input_tokens     INTEGER NOT NULL,
    output_tokens    INTEGER NOT NULL,
    occurred_at      TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_llm_usage_app ON llm_usage(application_id, occurred_at DESC);
CREATE INDEX idx_llm_usage_profile ON llm_usage(ai_profile_id, occurred_at DESC);
CREATE INDEX idx_llm_usage_exec ON llm_usage(job_execution_id);

CREATE TABLE llm_budgets (
    id         BLOB PRIMARY KEY,
    scope      TEXT NOT NULL,
    scope_id   BLOB,
    period     TEXT NOT NULL,
    limit_usd  REAL NOT NULL,
    hard_stop  INTEGER NOT NULL DEFAULT 0
);

-- migrate:down
