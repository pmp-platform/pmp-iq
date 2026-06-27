# Platform Inspector

Platform Inspector connects to one or more source-control accounts (GitHub,
GitLab, or local repositories), clones the repositories you select, runs
AI-driven analysis over them, and builds a queryable **platform model**:
applications, the languages and libraries they use, the infrastructure and
**tools** (docker compose, gradle, …) they build/run with, the external
**dependencies** they call — classified as cloud providers, services,
platforms/SaaS, or generic externals — how they connect to one another, and
who can access them: real repository members fetched from the provider (with
their permissions, and tracked as `member`/`ex_member` as people come and go)
alongside CODEOWNERS-derived `codeowner` grants. For each application it also
captures internal **components** and their **observability signals**, the
**use cases** they fulfil, AI-generated **mermaid diagrams** (rendered
locally), and the outbound **dependencies** detected from code (the apps/services
it connects to), each mapped to the component that makes the connection. Each
dependency's target name is resolved against the catalog of already-known
apps/services (exact → normalized → fuzzy), so connections link to existing
entities instead of fragmenting into near-duplicates. The `sync-repositories` job
refreshes all of this on each run, removing data a repo no longer produces. The result is browsable as filterable tables, an
interactive connection graph, and per-application detail pages.

> Status: all milestones implemented (see [`docs/`](docs/)). Verified by 75
> unit tests and 38 testcontainers-backed integration tests; ~85% line
> coverage.

## Architecture

- **Language / edition:** Rust 2024.
- **HTTP:** `axum` on Tokio.
- **Database:** SQLite by default (zero-config; schema auto-created at boot) or
  PostgreSQL when `DATABASE_URL` is a `postgres://` URL. Each repository trait has
  a Postgres and a SQLite implementation, selected from the engine at startup.
  PostgreSQL migrations are managed via **dbmate**.
- **UI:** server-rendered HTML (minijinja) enhanced with **jQuery** and styled
  with **Tailwind CSS**. All vendor JS/CSS is served locally from `assets/` —
  no CDNs at runtime.
- **Pluggable strategies:** repository providers (GitHub/GitLab/local), AI
  providers (Anthropic API / Claude CLI), and login strategies.

Every external dependency sits behind a trait, so it can be mocked in unit
tests; database-backed behaviour is covered by integration tests that spin up a
real PostgreSQL container via **testcontainers**.

## Prerequisites

- Rust (stable, edition 2024 capable) and Cargo.
- Docker (for the database, dbmate, and integration tests).

## Quick start

```bash
cp .env.example .env                 # adjust as needed

# Zero-config: with no DATABASE_URL, the app uses a local SQLite file and
# creates the schema automatically.
cargo run                            # serves on http://localhost:8080
```

To use PostgreSQL instead, set `DATABASE_URL` to a `postgres://` URL and apply
migrations with dbmate:

```bash
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/platform_inspector
bin/up.sh migrate                    # start Postgres + run dbmate (Windows: bin\up.bat migrate)
cargo run
```

If `ADMIN_USER` / `ADMIN_PASS` are unset, an `admin` user with a random password
is generated on boot and printed once to the logs.

## Configuration

All configuration comes from the environment; see [`.env.example`](.env.example)
for the full list. Key variables:

| Variable | Default | Purpose |
|----------|---------|---------|
| `DATABASE_URL` | `sqlite://platform_inspector.db?mode=rwc` | SQLite file (default) or a `postgres://` URL |
| `PORT` | `8080` | HTTP port |
| `ADMIN_USER` / `ADMIN_PASS` | — | Static admin login (generated if unset) |
| `SESSION_SECRET` | dev value | Session signing secret |
| `ENCRYPTION_KEY` | dev value | Base64 32-byte key for secrets at rest |
| `WORKSPACE_DIR` | `tmp/workspace` | Where repositories are cloned |

## Database & migrations

Migrations exist per engine: `db/migrations/` (PostgreSQL) and
`db/migrations_sqlite/` (SQLite), both dbmate format (up-only,
`--no-dump-schema`). The same SQL is also embedded in the binary.

- **SQLite** schema is applied automatically at boot (idempotent, tracked in
  `_app_migrations`) — nothing to run.
- **PostgreSQL** schema is applied with dbmate:

```bash
bin/up.sh migrate          # bring up Postgres and run `dbmate up`
dbmate new <name>          # create a new migration (leave migrate:down empty)
```

## Front-end assets

jQuery and Tailwind CSS are vendored under `assets/vendor/` and served from
`/assets` — the app makes **no external requests at runtime**. To refresh them:

```bash
curl -L https://code.jquery.com/jquery-3.7.1.min.js  -o assets/vendor/jquery.min.js
curl -L https://cdn.tailwindcss.com/3.4.16           -o assets/vendor/tailwind.js
curl -L https://unpkg.com/@antv/g6@5/dist/g6.min.js -o assets/vendor/g6.min.js
```

Shared page behaviour lives in `assets/app.js`.

## Testing

```bash
cargo test --lib           # fast unit tests (mocked dependencies, no Docker)
cargo test                 # full suite incl. testcontainers integration tests
```

Unit tests never touch external services. Integration tests run against both
engines: SQLite-backed tests use a temp file (no Docker), and Postgres-backed
tests start a disposable PostgreSQL container (Docker required) per the shared
harness in `tests/common/`.

## Security & operations

- **Auth on every route** — all pages and `/api/*` endpoints sit behind a
  session gate; unauthenticated API calls get `401`, pages redirect to `/login`.
- **CSRF** — the login form carries a per-session CSRF token, validated on POST.
- **Secrets at rest** — repository tokens and AI API keys are encrypted with
  AES-256-GCM (`ENCRYPTION_KEY`) and never returned by the API or written to
  logs. Session cookies are `HttpOnly` + `SameSite=Lax` (set `Secure` behind
  TLS).
- **Fail-fast config** — `Config::from_env()` and `AppState::build` validate
  configuration (including the encryption key) at startup.
- **Graceful shutdown** — the server drains in-flight requests on `Ctrl-C`.
- **Isolation** — each review job clones into its own workspace directory; AI
  and git access run behind traits with bounded retries and per-repository error
  isolation.
- **Rate limits & resumable jobs** — GitHub/GitLab calls are throttled
  (`GIT_API_MIN_INTERVAL_MS`); on a rate-limit response the review job
  **self-pauses**, persisting its checkpoint and a resume time from the
  rate-limit headers. Jobs can also be paused/resumed manually. A
  **leader-elected controller** (a TTL distributed lock, so only one instance
  acts when several run) resumes paused executions once their `resume_at`
  elapses.
  - **Checkpoint contents** — the review job persists a checkpoint into the
    execution's `state`: the set of accounts already fully processed plus a
    running tally (repositories, cloned, failed, analyzed, analysis-failed).
    On resume it skips finished accounts, so resuming is idempotent and does
    not re-clone completed work.
  - **Resume time** — derived from the rate-limit response headers, in
    precedence order: `retry-after` (relative seconds), then
    `x-ratelimit-reset` or `ratelimit-reset` (absolute Unix epoch). The result
    becomes the execution's `resume_at`.

## Walkthrough

1. Log in (admin from env, or the generated password printed at boot).
2. **Settings → Repository accounts:** add a GitHub/GitLab/local account, test
   the connection, preview the selected repositories.
3. **Settings → AI agent profiles:** add an Anthropic or Claude-CLI profile and
   test it.
   - **Settings → Entity kinds / Properties:** constrain the allowed `kind`
     vocabulary per entity (so the AI can't emit `vcs` and `vcs-api` for the same
     thing — out-of-list values become `other`) and define which properties the
     analyzer extracts into each entity's metadata. Both ship seeded with
     sensible defaults.
4. **Jobs:** create a `review-repositories` job (set `ai_profile_id` in its
   config to enable analysis), run it, and watch the execution logs.
5. **Platform:** a tabbed section (defaulting to the **Graph**, which shows
   applications only by default — toggle other entity kinds on via the legend) —
   browse applications, libraries, infrastructure, tools, cloud providers,
   services, platforms, external dependencies, users and groups as filterable
   tables. The application detail page is itself tabbed: an **Overview** pairing a
   focused connection graph (the app, its dependencies and infrastructure) with
   its properties (friendly names) and languages; a **Use cases** tab with
   interactive flowcharts (use cases → their components) plus the generated
   mermaid diagrams; per-relation tables (services, cloud providers, platforms,
   libraries, tools, external, components, observability signals — each shown
   only when present); and an always-present **Members** tab.

## Project layout

See [`CLAUDE.md`](CLAUDE.md) for the module map and conventions, and
[`docs/`](docs/) for the milestone specifications.
