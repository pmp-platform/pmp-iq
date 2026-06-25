# Milestone 04 — Settings: repository accounts

## Goal

Let an operator configure one or more **repository accounts** in a Settings
section, persisted in the database. Support multiple provider types via a
strategy pattern (GitHub, GitLab, local), with per-account repository selection
(all, regex, or explicit list).

## Scope

- Settings UI shell + the "Repository accounts" subsection.
- `repository_accounts` table and repository (data-access) trait.
- `RepositoryProvider` strategy trait: GitHub, GitLab, local.
- Repository selection (all / regex / explicit list) with a preview ("test
  connection / list repos").
- Encryption-at-rest for credentials.

## Deliverables

### Data model

```sql
-- migrate:up
CREATE TABLE repository_accounts (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    provider_type   TEXT NOT NULL,            -- 'github' | 'gitlab' | 'local'
    auth_type       TEXT NOT NULL,            -- 'token' | 'app' | 'none'
    base_url        TEXT,                     -- self-hosted GitLab/GHE; null = SaaS
    credentials_enc BYTEA,                    -- encrypted token / app config
    selection_mode  TEXT NOT NULL,            -- 'all' | 'regex' | 'list'
    selection_value TEXT,                     -- regex pattern or JSON list
    enabled         BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- migrate:down
```

### Provider strategy

- `RepositoryProvider` trait:
  - `list_repositories(&self) -> Result<Vec<RemoteRepo>, ProviderError>`
  - `validate(&self) -> Result<(), ProviderError>` (test credentials).
  - `RemoteRepo` = name, full name, clone URL, default branch, visibility.
- Implementations: `GitHubProvider`, `GitLabProvider`, `LocalProvider`
  (filesystem path scan). HTTP access goes through an injected
  `HttpClient` trait so providers are unit-testable with mocked responses.
- A `RepositoryProviderFactory` builds the right provider from an account row,
  decrypting credentials.

### Selection

- A reusable `RepoSelector` applies `selection_mode` to a provider's repo list:
  `all` (everything), `regex` (filter by pattern), `list` (explicit names).
  Centralised, unit-tested, reused by the cloning job (M07).

### Credential encryption

- Symmetric encryption (key from config/secret) behind an `Encryptor` trait;
  store ciphertext in `credentials_enc`. Decrypt only when building a provider.
  Mocked in unit tests.

### UI

- Settings → Repository accounts: list, create, edit, delete, enable/disable.
- "Test connection" calls `validate()`; "Preview repositories" shows the selected
  subset using `RepoSelector`. jQuery handles the async calls and rendering.

## Tasks

- [ ] Migration for `repository_accounts`.
- [ ] `RepositoryAccountRepository` trait + sqlx impl + mock.
- [ ] `RepositoryProvider` trait + GitHub/GitLab/local impls over an `HttpClient`
      trait; `RepositoryProviderFactory`.
- [ ] `RepoSelector` (all/regex/list) with unit tests.
- [ ] `Encryptor` trait + impl; encrypt on save, decrypt on use.
- [ ] CRUD API endpoints + Settings UI subsection with test/preview actions.
- [ ] Unit tests for providers (mocked HTTP), selector, and encryption.

## Acceptance criteria

- An operator can add a GitHub account with a token, test the connection, and
  preview the matching repositories using each selection mode.
- GitLab and local accounts work through the same UI and trait.
- Credentials are stored encrypted; plaintext never persisted or logged.
- Provider and selector logic is unit-tested with mocked HTTP — no live calls.

## Dependencies

Milestones 01–03.

## Out of scope

Cloning the repositories (M07) and analysing them (M08).
