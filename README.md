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

### GitHub login

Set `auth.provider: github` (or `AUTH_PROVIDER=github`) to authenticate via
GitHub instead of the static admin account. Two modes (`auth.github.mode`):

- **`oauth_app`** — a "Sign in with GitHub" web flow backed by a GitHub App /
  OAuth app (`client_id` + `client_secret` + `redirect_url`).
- **`personal_token`** — the user enters their GitHub username and a personal
  access token at the login form.

In both modes the user must pass an allowlist (`allowed_orgs` and/or
`allowed_logins`); with both empty, no one is authorised. The admin account
remains as a fallback. See [`config.example.yaml`](config.example.yaml).

## Configuration

Configuration is layered: an **optional `config.yaml`** provides values, the
**process environment** overrides them, and built-in **defaults** fill the rest
(`env > file > default`). With no file present, behaviour is pure env + defaults.

- The file is looked up next to the binary, then in the working directory; the
  path is overridable with `--config-file <path>` (a missing explicit path is a
  hard error, a missing default-location file is simply absent).
- Any file value can pull from the environment with `${VAR}` or
  `${VAR:-default}`, so secrets stay outside the file. See
  [`config.example.yaml`](config.example.yaml) for the full schema.

```bash
platform-inspector --config-file /etc/platform-inspector/config.yaml
```

Key settings (env var | `config.yaml` path):

| Setting | Default | Purpose |
|---------|---------|---------|
| `DATABASE_URL` \| `database.url` | `sqlite://platform_inspector.db?mode=rwc` | SQLite file (default) or a `postgres://` URL |
| `PORT` \| `server.port` | `8080` | HTTP port |
| `REDIS_ENABLED` \| `redis.enabled` | `false` | Use Redis to back the distributed lock |
| `REDIS_URL` \| `redis.url` | `redis://localhost:6379` | Redis connection URL |
| `AUTH_PROVIDER` \| `auth.provider` | `admin` | Login provider: `admin` or `github` |
| `ADMIN_USER` / `ADMIN_PASS` \| `auth.*` | — | Static admin login (generated if unset) |
| `SESSION_SECRET` \| `auth.session_secret` | dev value | Session signing secret |
| `ENCRYPTION_KEY` \| `auth.encryption_key` | dev value | Base64 32-byte key for secrets at rest |
| `LOG_LEVEL` \| `log.level` | `info` | Log level (`RUST_LOG` still wins) |
| `WORKSPACE_DIR` \| `workspace_dir` | `tmp/workspace` | Where repositories are cloned |

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

## Running with Docker

A multi-stage [`Dockerfile`](Dockerfile) builds a slim runtime image. Two compose
topologies are provided, selected by the first argument to `bin/up.sh`:

```bash
bin/up.sh single        # one app container on SQLite, no dependencies
bin/up.sh distributed   # two app instances + nginx + Postgres + Redis
bin/down.sh single      # tear a topology down (named volumes persist)
```

- **single** — `docker-compose.single.yml` runs one container reading
  `deploy/single/config.yaml`; the SQLite file and cloned workspaces persist in
  the `app_data` volume. App served on `:8080`.
- **distributed** — `docker-compose.distributed.yml` runs `app1` + `app2` behind
  nginx (`:8080`), sharing Postgres + Redis (`deploy/distributed/config.yaml`,
  `redis.enabled: true`). dbmate applies the schema before the apps start. With
  two instances, exactly one holds the controller-leader lease and jobs/locks
  serialise across instances through the shared Redis lock. nginx uses `ip_hash`
  so a client stays pinned to one instance (the session store is in-memory).

Set `ENCRYPTION_KEY` (and, for distributed, `SESSION_SECRET`) in your shell
before `bin/up.sh` for non-default secrets; both are passed through to the
containers when present, and **must be identical** across the distributed
instances. Without them the app's built-in development defaults apply.

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
   its properties (friendly names) and languages; a **Use cases** tab showing a
   flowchart of use cases — clicking one opens a wide modal with its **Sequence
   diagram** and **Component diagram** (mermaid, generated per use case, with
   zoom/reset controls); per-relation tables (services, cloud providers, platforms,
   libraries, tools, external, components, observability signals — each shown
   only when present); and an always-present **Members** tab.

## Jobs, distributed locks & LLM features

- **Distributed locks** (`src/locks/`): a `DistributedLock` trait — a named lock
  with a TTL that can be **refreshed** — with an in-memory backend (the default
  for single-instance/SQLite) and a SQL-backed backend over `controller_locks`
  for multi-instance deployments. Used for controller leader election and to
  serialise jobs by resource key.
- **Scheduling & resilience:** jobs carry a `next_run_at` time polled by the
  leader-elected controller. A job that can't run right now (e.g. it can't take
  its lock) returns `JobError::CannotRun { retry_at }`; the runner **reschedules**
  it (at `retry_at`, or `now + 5m`) and records the execution as `skipped` rather
  than failed. Each job runs in its own workspace
  `{WORKSPACE_DIR}/jobs/{job-name}/{job-id}` (persisted across runs, so clones are
  reused). The `sync-repositories` job takes a per-job lock.
- **Liveness heartbeat:** the runner heartbeats each execution from a background
  task for its whole run, so a long-but-healthy job (a slow sync, a long LLM
  call) is never cancelled mid-run. The leader controller cancels executions
  whose heartbeat is older than 5 minutes — which only happens once the worker
  process/runtime has died (its heartbeat task can no longer beat); the runner
  won't overwrite a stale-cancelled execution with a terminal status.
- **Live output & metadata:** executions stream raw `output` (the `logs` column)
  and a `metadata` JSON object that jobs update while running. Any job that uses
  an LLM wraps its provider in a recorder that writes the **full prompt + response**
  to the output and accumulates **token usage** into the metadata.
- **`llm-repository-request` job:** clones (or fetches + rebases) a repository and
  runs an LLM session over the checkout with a supplied input, serialised by a
  per-repository lock. The answer and token usage are returned in the execution.
- **Per-application Sync:** when an application has a configured repository, its
  detail page shows a **Sync** button that schedules a `sync-repositories` run
  scoped to just that repository (via a `repository_id` execution param).
- **Ask the LLM about an application:** the application detail page has a prompt
  box that queues an `llm-repository-request` for the app's repository and keeps
  a **history** of the questions asked / being processed, polling until answered.
- **LLM hints** (`src/hints/`): every section of the application detail (Overview,
  Use cases, Components, libraries/services/…, and inside each use case) has an
  *LLM Hints* button that records free-text corrections scoped to the entity type
  or a specific entity. Hints are keyed by the entity's natural name (so they
  survive re-syncs) and are injected into the analysis prompt as authoritative
  corrections on the next sync.
- **File attribution & File Explorer:** the analyzer records which repository
  files each use case and component affects; the application detail page has a
  *File Explorer* **tab** with a lazy file tree of the cloned checkout, a
  read-only **CodeMirror** viewer with per-language syntax highlighting, and
  **Markdown rendering** (CodeMirror + marked vendored in `assets/vendor/`). File
  access is sandboxed to the checkout root (path traversal is rejected).
- **AI Agent tasks** (`src/agent_tasks/`): the application detail page has an
  *AI Agent* **tab** for creating change **tasks**. Each task is a multi-turn
  session with an agentic AI (the Claude CLI provider) that, per turn, checks out
  a dedicated `agent/<id>` branch, edits files, commits, pushes, and opens (or
  updates) a **pull request** — serialised per repository with a distributed lock.
  Follow-up messages refine the change on the same branch/PR; the transcript and
  live PR link are shown in a chat-style view. Requires a writable repository and
  a CLI-capable AI profile (PR creation is GitHub-only).
- **Job execution details:** the execution page streams live output and has a
  *More Details* modal showing the full record (summary, metadata, params, state)
  as prettified JSON.

## Project layout

See [`CLAUDE.md`](CLAUDE.md) for the module map and conventions, and
[`docs/`](docs/) for the milestone specifications.
