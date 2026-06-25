# Milestone 01 — Database layer & migrations (dbmate)

## Goal

Establish the persistence layer: dbmate migrations, a connection pool, and the
**repository trait** pattern that every later feature uses to read and write
data. Prove the layer end to end with a small, real table behind a mockable
interface.

## Scope

- dbmate migration workflow (up-only, no schema dump).
- `sqlx` connection pool supporting Postgres (default) and SQLite.
- A generic repository abstraction so storage is mockable in unit tests.
- A first `app_settings` key/value table used by later milestones for misc
  persisted configuration.

## Deliverables

### Migration workflow

- Migrations live in `db/migrations/`, created with
  `dbmate new <name>`.
- Every migration includes the `-- migrate:up` and `-- migrate:down`
  delimiters, but **`down` is intentionally empty** (project rule).
- dbmate runs with `--no-dump-schema`.
- Document the commands in `README.md`:
  - `dbmate up`, `dbmate new <name>` (run via the Compose `dbmate` service /
    profile from M00).

### Connection layer

- `Database` wrapper around a `sqlx` pool. A `Database::connect(&DatabaseConfig)`
  function chooses Postgres or SQLite from the configured driver and returns
  `Result<Database, DbError>`.
- Pool size, timeouts come from config.

### Repository pattern (the important part)

- Define a **trait per aggregate**, e.g. `SettingsRepository`, with async
  methods returning `Result<T, RepoError>`.
- Provide a `sqlx`-backed implementation (`SqlxSettingsRepository`).
- Unit tests use an **in-memory mock** implementing the trait — never the real
  database.
- A shared `RepoError` (via `thiserror`) maps `sqlx` errors to domain errors so
  callers never see driver types.

### First table

```sql
-- migrate:up
CREATE TABLE app_settings (
    key         TEXT PRIMARY KEY,
    value       JSONB NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- migrate:down
```

## Tasks

- [ ] Add `sqlx` (with `postgres`, `sqlite`, `runtime-tokio`, `macros`).
- [ ] Implement `Database::connect` selecting driver from config.
- [ ] Create the `app_settings` migration via dbmate.
- [ ] Define `SettingsRepository` trait + `SqlxSettingsRepository`.
- [ ] Implement a `MockSettingsRepository` for tests (or use `mockall`).
- [ ] Wire the pool into the app builder from M00.
- [ ] Add `cargo` integration-test guidance (DB-backed tests are integration
      tests, gated separately from unit tests; unit tests stay mock-only).

## Acceptance criteria

- `dbmate up` applies migrations to the Compose Postgres with no schema dump.
- The app connects to Postgres at boot; `/healthz` reports DB connectivity.
- `SettingsRepository` get/set round-trips against a real DB in an integration
  test, and has unit tests against the mock with no real DB access.
- Switching `DATABASE_DRIVER` to SQLite still boots and passes the settings
  integration test.

## Dependencies

Milestone 00 (config, Docker, scripts).

## Out of scope

Domain tables (accounts, jobs, platform model) — each later milestone owns its
own migrations.
