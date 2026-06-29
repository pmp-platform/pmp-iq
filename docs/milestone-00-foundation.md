# Milestone 00 — Project foundation & tooling

## Goal

Stand up the empty-but-runnable skeleton: a Rust 2024 project that builds, a
configuration layer, a Docker-based Postgres, dbmate wired up, the helper
scripts, and baseline documentation. Nothing user-facing yet — this milestone
makes every later milestone fast to start.

## Scope

- Cargo project (Rust 2024 edition) with a clean module layout.
- Layered configuration loaded from environment with sane defaults.
- `docker-compose.yml` providing Postgres (and a `dbmate` service).
- `bin/up.*` / `bin/down.*` helper scripts.
- Logging/tracing initialised at boot.
- Baseline `README.md` and `CLAUDE.md`.

## Deliverables

### Project layout

```
platiq/
├── Cargo.toml                 # edition = "2024"
├── docker-compose.yml
├── .env.example
├── bin/
│   ├── up.sh    up.bat
│   └── down.sh  down.bat
├── db/
│   └── migrations/            # dbmate migrations (added in M01)
├── assets/                    # local jQuery/Tailwind/graph assets (M02+)
├── docs/                      # these milestone files
└── src/
    ├── main.rs                # entrypoint only: config -> services -> serve
    ├── config.rs              # Config struct + loader
    ├── telemetry.rs           # tracing/log init
    └── app.rs                 # application wiring (builder)
```

### Configuration

- A `Config` struct holding: database URL/driver, bind address/port, asset
  directory, admin credentials (optional), session secret. Loaded from env via a
  single `Config::from_env()` returning `Result<Config, ConfigError>`.
- Group related settings into nested structs (`DatabaseConfig`, `ServerConfig`,
  `AuthConfig`) so no constructor exceeds four parameters.
- `.env.example` documents every variable.

### Tooling

- `docker-compose.yml` with a `db` service (Postgres 16) and a `dbmate` service
  (`amacneil/dbmate:2.28.0`) using a Compose **profile** so it only runs on
  demand. Support an optional profile argument end-to-end.
- `bin/up.sh` / `bin/up.bat`: call the matching `down` script, then
  `docker compose up`, forwarding an optional profile argument.
- `bin/down.sh` / `bin/down.bat`: run `docker compose rm -f --all`, forwarding an
  optional profile argument.

## Tasks

- [ ] `cargo init` with edition 2024; add `tokio`, `axum` (placeholder server),
      `tracing`, `tracing-subscriber`, `serde`, `thiserror`, `anyhow`.
- [ ] Implement `Config` with nested config structs and `from_env()`.
- [ ] Implement `telemetry::init()` for structured logging.
- [ ] `main.rs` only: load config, init telemetry, build app, run.
- [ ] Author `docker-compose.yml` with `db` + profiled `dbmate` services.
- [ ] Write `bin/up.*` and `bin/down.*` (profile arg supported).
- [ ] Write `.env.example`, baseline `README.md`, baseline `CLAUDE.md`.

## Acceptance criteria

- `cargo build` and `cargo clippy` succeed with no warnings.
- `bin/up.sh` brings up Postgres; `bin/down.sh` tears it down. Both accept a
  profile argument.
- Running the binary loads config from env, logs a startup line, and binds a
  health endpoint (`GET /healthz` → `200`).
- `Config::from_env()` has unit tests covering defaults and overrides (env access
  mocked behind a small `EnvSource` trait — no real env reads in tests).

## Out of scope

Database schema, HTTP routes beyond `/healthz`, UI, auth.
