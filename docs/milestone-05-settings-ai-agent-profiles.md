# Milestone 05 — Settings: AI agent profiles

## Goal

Let an operator define reusable **AI agent profiles** in Settings, persisted in
the database. Each profile names an AI provider and carries provider-specific
configuration. Support two providers via a strategy pattern: the **Anthropic
API** and the **Claude CLI binary**.

## Scope

- `ai_agent_profiles` table + data-access trait.
- `AiProvider` strategy trait with Anthropic and Claude-CLI implementations.
- Per-provider config schemas (model, effort, etc.).
- Settings subsection with CRUD and a "test prompt" action.

## Deliverables

### Data model

```sql
-- migrate:up
CREATE TABLE ai_agent_profiles (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT NOT NULL,
    provider_type TEXT NOT NULL,        -- 'anthropic' | 'claude_cli'
    config        JSONB NOT NULL,       -- provider-specific (model, effort, ...)
    secrets_enc   BYTEA,                -- encrypted API key when applicable
    enabled       BOOLEAN NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- migrate:down
```

### Provider strategy

- `AiProvider` trait:
  - `complete(request: AiRequest) -> Result<AiResponse, AiError>` where
    `AiRequest` bundles prompt/system/context (a struct, not many params) and
    `AiResponse` carries text + token/usage metadata.
  - `validate(&self) -> Result<(), AiError>`.
- `AnthropicProvider`: calls the Claude API over the injected `HttpClient` trait.
  Config: `model` (default to a current Claude model id), `max_tokens`,
  `temperature`, optional `effort`/thinking settings. API key from
  `secrets_enc` (decrypted via the `Encryptor` from M04). **Before implementing,
  consult the `claude-api` reference for current model ids and parameters.**
- `ClaudeCliProvider`: invokes the local `claude` binary through an injected
  `CommandRunner` trait (so process execution is mocked in tests). Config:
  binary path, `model`, `effort`, extra flags. Parses CLI output into
  `AiResponse`.
- An `AiProviderFactory` builds the right provider from a profile row.

### Config schemas

- Each provider declares its config shape (typed structs) and validates incoming
  JSON. Unknown/invalid config is rejected with a clear error.

### UI

- Settings → AI agent profiles: list, create, edit, delete, enable/disable.
- The create/edit form adapts fields to the selected provider (jQuery).
- "Test profile" sends a fixed prompt through `complete()` and shows the result.

## Tasks

- [ ] Migration for `ai_agent_profiles`.
- [ ] `AiProfileRepository` trait + sqlx impl + mock.
- [ ] `AiProvider` trait; `AnthropicProvider` (over `HttpClient`) and
      `ClaudeCliProvider` (over `CommandRunner`); `AiProviderFactory`.
- [ ] Typed per-provider config structs + validation.
- [ ] Reuse the `Encryptor` for API keys.
- [ ] CRUD API + Settings subsection with provider-aware form and test action.
- [ ] Unit tests: Anthropic with mocked HTTP, Claude CLI with mocked command
      runner, config validation.

## Acceptance criteria

- An operator can create an Anthropic profile (model/effort) and a Claude-CLI
  profile, and run a successful "test profile" for each.
- API keys are stored encrypted and never logged.
- Both providers satisfy one `AiProvider` trait so the jobs layer (M08) depends
  only on the trait. Provider logic is unit-tested with mocked HTTP/command
  execution.

## Dependencies

Milestones 01–04 (encryption, settings shell, HTTP client trait).

## Out of scope

Using profiles inside jobs (M06–M08).
