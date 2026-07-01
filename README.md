# pmp-iq

**pmp-iq** is an AI-powered software/platform catalog. It connects to your
source-control accounts (GitHub, GitLab, or local repositories), clones the
repositories you select, runs LLM-driven analysis over their code, and builds a
queryable **platform model** of your entire fleet — the applications, the
languages and libraries they use, the infrastructure and tooling they run on,
the external services they depend on, how everything connects, and who owns and
can access each piece.

The result is browsable as filterable tables, an interactive connection graph,
C4 architecture diagrams, per-application detail pages, and an insights
dashboard — and it can answer natural-language questions about your platform,
open AI-authored pull requests across many repos at once, and keep itself up to
date from webhooks.

> **Status:** all milestones (see [`docs/`](docs/)) are implemented and covered
> by mocked unit tests plus testcontainers-backed integration tests against both
> SQLite and PostgreSQL.

## Table of contents

- [What it builds](#what-it-builds)
- [Features](#features)
- [Architecture](#architecture)
- [Prerequisites](#prerequisites)
- [Quick start (local)](#quick-start-local)
- [GitHub login](#github-login)
- [Configuration](#configuration)
- [Database & migrations](#database--migrations)
- [Running with Docker](#running-with-docker)
- [Front-end assets](#front-end-assets)
- [Testing](#testing)
- [Security & operations](#security--operations)
- [Walkthrough](#walkthrough)
- [Project layout](#project-layout)

## What it builds

From each analysed repository, pmp-iq extracts and continuously refreshes a
connected model:

- **Applications** — with their detected type, languages, and friendly
  properties.
- **Languages & libraries** — the ecosystems and dependencies each app uses,
  de-duplicated and shared across the fleet.
- **Infrastructure & tools** — what each app builds/runs with (docker compose,
  gradle, …).
- **External dependencies** — the apps/services each app calls, classified as
  cloud providers, services, platforms/SaaS, or generic externals. Each
  dependency's free-form target name is resolved against the catalog of
  already-known entities (exact → normalized → fuzzy), so connections link to
  existing entities instead of fragmenting into near-duplicates.
- **People & access** — real repository members fetched from the provider (with
  their permissions, tracked as `member`/`ex_member` as people come and go),
  alongside CODEOWNERS-derived `codeowner` grants.
- **Per-application internals** — components and their observability signals, the
  use cases each app fulfils, AI-generated **mermaid** sequence and component
  diagrams (rendered locally), a **codebase map**, and the outbound dependencies
  detected from code, each mapped to the component that makes the connection.

Every sync removes data a repository no longer produces and prunes shared
entities nothing references any more, so the model stays an accurate reflection
of the code.

## Features

### Catalog & exploration
- **Interactive connection graph** (AntV G6) — applications by default, with
  other entity kinds toggled on via the legend.
- **Filterable, searchable, paginated tables** for every entity kind
  (applications, libraries, infrastructure, tools, cloud providers, services,
  platforms, external dependencies, users, groups) with per-entity filter
  facets.
- **Tabbed application detail pages** — Overview (focused app → dependencies →
  infrastructure graph + properties/languages), Use cases (flowchart → click for
  per-use-case sequence + component diagrams), Interactions (outbound
  http/db/queue/… calls, each drillable to the implementing component's files),
  conditional per-relation tables, a File Explorer, and an always-present
  Members tab.
- **Codebase map** — a bounded directory/module structure graph derived from the
  cloned checkout, rendered as an interactive tree.
- **C4 model views & export** — projects the platform graph into Structurizr
  DSL and C4 mermaid diagrams at three levels: **Context** (fleet system
  landscape, applications by default; dependencies opt-in), **Container** (one
  application's datastores/services + external boundaries) and **Component**
  (its internal components, with inter-component and component→external edges).
  Drill Context → Container → Component from the C4 tab.
- **Insights dashboard** — fleet rollups, leaderboards (best/worst coverage and
  complexity), and group-by breakdowns over the latest quality metrics, plus
  **metric trends & charts**: coverage/complexity trend lines over time,
  distribution histograms, a coverage-vs-complexity scatter (bubble = LOC), a
  portfolio treemap, and per-application sparklines — all drawn with a locally
  vendored chart renderer (no CDN).
- **LLM cost & budgeting** — every recorded LLM call is priced (a configurable
  per-model price map) and rolled up by application, AI profile, job type and
  period. Spend budgets (global/profile/job/application, daily/monthly) warn or
  hard-stop work that would exceed the limit (rescheduling via the existing
  `CannotRun` path), shown in the Insights cost panel.
- **Configurable extraction prompts** — each analysis section (applications,
  components, use cases, dependencies, members, diagrams, observability) and the
  metrics-collection prompt is individually editable in Settings, with the strict
  kind/property vocabulary and JSON schema always injected so edits can never
  break extraction. Disable a section to omit it; reset restores the default.
- **Semantic search & duplicate detection** — catalog entities (applications,
  components, use cases) are embedded so you can search by meaning ("apps that
  send email"), find "similar applications", and surface "possible duplicates"
  (similarity clusters). Search falls back to substring matching when embeddings
  aren't configured; generation re-embeds only changed entities.
- **Change timeline & audit** — each sync records precise model changes
  (applications and dependencies created/updated/removed, keyed by stable natural
  keys) as a per-application and platform-wide timeline, with a two-point diff
  summary. Mutating operator actions (logins, prompt edits, …) are recorded in an
  admin-only audit log.
- **RBAC, teams & multi-tenant** — roles (viewer / maintainer / admin) gate
  actions; teams own applications so maintainers can only act on apps their team
  owns; admins manage teams and roles. An optional, feature-flagged multi-tenant
  mode scopes the visible application catalog to the caller's tenant.
- **Version currency & tech radar** — each app's dependencies are assessed
  against a configurable policy (how many majors behind, end-of-life status),
  fleet currency is ranked, and a tech radar (adopt/trial/assess/hold) is curated.
- **Operator gamification** — XP, levels, skills and badges, derived purely from
  the recorded action/audit history (no new tracking), with a per-operator profile
  and a leaderboard.
- **Production-readiness scorecards** — configurable checks (owner, coverage, CI,
  tests, vulns, observability, docs) are evaluated against each app's model +
  metrics + ownership into a weighted score and a bronze/silver/gold/at-risk
  level, per application and ranked across the fleet.
- **API contracts** — each application's exposed HTTP/gRPC/GraphQL operations are
  extracted, and outbound dependencies resolve to the producer's specific
  endpoint, so you can see an endpoint's consumers (impact) on the app's API panel.
- **DORA metrics** — deployment and incident events (captured via GitHub
  `deployment_status`/`release` webhooks or a generic event API) derive the four
  DORA measures (deployment frequency, lead time, change-failure rate, MTTR) and
  a performance tier per application and fleet-wide, recorded as trending metrics.
- **Incremental analysis** — a scoped re-sync (webhook push or a scheduled run)
  diffs the changed files since the last analyzed commit and re-analyzes only the
  affected components/use cases, merging them into the existing model. Structural
  changes (manifests / CI / unreachable base commit) safely fall back to a full
  analysis; full-fleet manual syncs stay full.

### AI analysis
- **LLM-driven repository analysis** that produces the entire platform model,
  driven by a configurable analysis vocabulary (allowed entity **kinds** and
  extracted **properties**) so the model the AI emits is constrained and
  consistent.
- **Configurable AI providers** — the **Anthropic Messages API** or the local
  **Claude CLI**, each behind a common trait, with a selectable model and
  reasoning effort.
- **LLM hints** — free-text corrections scoped to an entity type or a specific
  entity, keyed by natural name so they survive re-syncs and are injected into
  the analysis prompt as authoritative corrections.
- **Quality metrics** — an LLM extracts categorised signals per repository across
  **code health** (tests/coverage/complexity/LOC/CI, duplication, lint, TODOs, doc
  coverage, convention compliance) and **security/supply chain** (vulnerabilities,
  outdated/secret deps), complemented by **derived** architecture and
  model-coverage metrics computed from the platform model (no LLM). History is kept
  and feeds the dashboard; the per-app Insights panel groups metrics by category.
- **Ask the platform** — a natural-language question answered against a
  serialised snapshot of the whole catalog (the model is forbidden from
  inventing data).
- **Ask about an application** — a per-app Q&A box that runs an LLM session over
  the app's cloned checkout and keeps a history of answers.

### Automated changes
- **AI Agent tasks** — multi-turn change tasks where an agentic Claude CLI
  checks out a dedicated branch, edits files, commits, pushes, and opens/updates
  a **pull request** — per single application or **fanned out across many
  repositories** at once.
- **Batch-change campaigns** — a named change applied across the fleet (explicit
  apps or an allowlist filter), driving one multi-repo agent task with per-repo
  PR progress.
- **PR watcher** — polls open PRs, finishes merged/closed ones, and on new review
  comments / merge conflicts / failed checks enqueues an agent fix turn.
- **Auto-remediation** — operator-defined rules map a finding (metric below/above
  a threshold, a failed scorecard check, or an end-of-life dependency) to a
  remediation. An on-demand evaluation sweeps the fleet and proposes deduplicated
  remediations; approving one opens an agent task for the affected application.
- **Webhooks** — HMAC-verified GitHub webhooks trigger immediate PR reconciles
  and merge-driven re-syncs.

### Jobs, scheduling & resilience
- **Jobs subsystem** — `sync-repositories`, `llm-repository-request`,
  `application-agent-task`, `pr-watcher`, and `collect-metrics`, with live
  streaming output and per-execution metadata.
- **Cron scheduling**, manual run/pause/resume, **resumable checkpoints** on
  rate-limit, a **liveness heartbeat** so healthy long jobs are never cancelled,
  and **configurable concurrency** with queueing.
- **Leader-elected controller** over a TTL **distributed lock** (in-memory, SQL,
  or Redis backend) that drives scheduling and stale-execution recovery across
  multiple instances.

### Platform & operations
- **Dual database** — SQLite (zero-config default) or PostgreSQL, same code path.
- **Pluggable auth** — static admin account or **GitHub login** (OAuth App web
  flow or personal token) with org/login allowlists.
- **Secrets encrypted at rest** (AES-256-GCM), CSRF-protected login, auth on
  every route.
- **No CDNs at runtime** — all front-end vendor JS/CSS is served locally.
- **Docker topologies** — single-instance (SQLite) or distributed (two apps +
  nginx + Postgres + Redis).

## Architecture

- **Language / edition:** Rust 2024.
- **HTTP:** `axum` on Tokio.
- **Database:** SQLite by default (zero-config; schema auto-created at boot) or
  PostgreSQL when `DATABASE_URL` is a `postgres://` URL. Each repository trait has
  a Postgres and a SQLite implementation, selected from the engine at startup.
  PostgreSQL migrations are managed via **dbmate**.
- **UI:** server-rendered HTML (minijinja) enhanced with **jQuery** and styled
  with **Tailwind CSS**; graphs via **AntV G6**, diagrams via **mermaid**, code
  viewing via **CodeMirror** + **marked**. All vendor JS/CSS is served locally
  from `assets/` — no CDNs at runtime.
- **Pluggable strategies:** repository providers (GitHub/GitLab/local), AI
  providers (Anthropic API / Claude CLI), and login strategies.

Every external dependency sits behind a trait, so it can be mocked in unit
tests; database-backed behaviour is covered by integration tests that spin up a
real PostgreSQL container via **testcontainers**.

## Prerequisites

- Rust (stable, edition 2024 capable) and Cargo.
- Docker — only for PostgreSQL, dbmate, and the Postgres-backed integration
  tests. **Not** needed for the zero-config SQLite quick start.
- An AI provider to enable analysis: an Anthropic API key, or the `claude` CLI
  installed locally. (Cloning works without one; analysis is skipped until a
  profile exists.)

## Quick start (local)

```bash
cp .env.example .env                 # adjust as needed

# Zero-config: with no DATABASE_URL, the app uses a local SQLite file and
# creates the schema automatically.
cargo run                            # serves on http://localhost:8080
```

If `ADMIN_USER` / `ADMIN_PASS` are unset, an `admin` user with a random password
is generated on boot and printed once to the logs — use it to sign in.

To use **PostgreSQL** instead, set `DATABASE_URL` to a `postgres://` URL and
apply migrations with dbmate:

```bash
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/pmp-iq
bin/up.sh migrate                    # start Postgres + run dbmate (Windows: bin\up.bat migrate)
cargo run
```

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
pmp-iq --config-file /etc/pmp-iq/config.yaml
```

Key settings (env var | `config.yaml` path):

| Setting | Default | Purpose |
|---------|---------|---------|
| `DATABASE_URL` \| `database.url` | `sqlite://pmp-iq.db?mode=rwc` | SQLite file (default) or a `postgres://` URL |
| `PORT` \| `server.port` | `8080` | HTTP port |
| `BIND_ADDRESS` \| `server.bind_address` | `0.0.0.0` | Bind address |
| `REDIS_ENABLED` \| `redis.enabled` | `false` | Use Redis to back the distributed lock |
| `REDIS_URL` \| `redis.url` | `redis://localhost:6379` | Redis connection URL |
| `AUTH_PROVIDER` \| `auth.provider` | `admin` | Login provider: `admin` or `github` |
| `ADMIN_USER` / `ADMIN_PASS` \| `auth.*` | — | Static admin login (generated if unset) |
| `SESSION_SECRET` \| `auth.session_secret` | dev value | Session signing secret |
| `ENCRYPTION_KEY` \| `auth.encryption_key` | dev value | Base64 32-byte key for secrets at rest |
| `WEBHOOK_GITHUB_SECRET` \| `webhooks.github_secret` | — | Shared secret for GitHub webhook signatures |
| `AGENT_MAX_CONCURRENCY` \| `agent.max_concurrency` | `4` | Parallel AI-Agent turns |
| `GIT_API_MIN_INTERVAL_MS` \| `git_min_interval_ms` | `250` | Min interval between GitHub/GitLab API calls |
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

All third-party JS/CSS is vendored under `assets/vendor/` and served from
`/assets` — the app makes **no external requests at runtime**. To refresh them:

```bash
curl -L https://code.jquery.com/jquery-3.7.1.min.js   -o assets/vendor/jquery.min.js
curl -L https://cdn.tailwindcss.com/3.4.16            -o assets/vendor/tailwind.js
curl -L https://unpkg.com/@antv/g6@5/dist/g6.min.js   -o assets/vendor/g6.min.js
curl -L https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js -o assets/vendor/mermaid.min.js
curl -L https://cdn.jsdelivr.net/npm/marked/marked.min.js          -o assets/vendor/marked.min.js
```

CodeMirror (`codemirror.min.js`/`.css` + `codemirror-modes.min.js`) powers the
read-only file viewer. Shared page behaviour lives in `assets/app.js`.

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
  Public exceptions are the login and OAuth-callback routes and the
  HMAC-verified `POST /webhooks/github`.
- **CSRF** — the login form carries a per-session CSRF token, validated on POST.
- **Secrets at rest** — repository tokens and AI API keys are encrypted with
  AES-256-GCM (`ENCRYPTION_KEY`) and never returned by the API or written to
  logs. Session cookies are `HttpOnly` + `SameSite=Lax` (set `Secure` behind
  TLS).
- **Fail-fast config** — configuration (including the encryption key) is
  validated at startup in `AppState::build`.
- **Graceful shutdown** — the server drains in-flight requests on `Ctrl-C`.
- **Isolation** — each job clones into its own workspace directory; AI and git
  access run behind traits with bounded retries and per-repository error
  isolation.
- **Rate limits & resumable jobs** — GitHub/GitLab calls are throttled
  (`GIT_API_MIN_INTERVAL_MS`); on a rate-limit response the review job
  **self-pauses**, persisting its checkpoint and a resume time from the
  rate-limit headers. Jobs can also be paused/resumed manually. The
  **leader-elected controller** resumes paused executions once their `resume_at`
  elapses.
  - **Checkpoint contents** — the review job persists into the execution's
    `state` the set of accounts already fully processed plus a running tally
    (repositories, cloned, failed, analyzed, analysis-failed). On resume it skips
    finished accounts, so resuming is idempotent and does not re-clone completed
    work.
  - **Resume time** — derived from the rate-limit response headers, in
    precedence order: `retry-after` (relative seconds), then `x-ratelimit-reset`
    or `ratelimit-reset` (absolute Unix epoch).

## Walkthrough

1. **Log in** — admin from env, or the generated password printed at boot.
2. **Settings → Repository accounts** — add a GitHub/GitLab/local account, test
   the connection, preview the selected repositories. Optionally set an
   **organization / group**: for the **All**/regex modes it filters the token's
   accessible repositories to that namespace (`org/…`, subgroups included); for
   the **Specific repositories** mode each entry is fetched directly
   (`owner/name`, or a bare `name` prefixed with the organization), so repos you
   can reach as an outside collaborator with a personal token are found even
   when they don't appear in a listing. Left blank, behaviour is unchanged.
3. **Settings → AI agent profiles** — add/edit/validate an Anthropic or
   Claude-CLI profile (each exposes a `model` and a reasoning `effort`:
   `low`/`medium`/`high`/`xhigh`/`max`). An Anthropic profile requires an API key;
   for Claude-CLI it is optional. When editing, leave the key blank to keep the
   stored one.
4. **Settings → Entity kinds / Properties** — constrain the allowed `kind`
   vocabulary per entity and define which properties the analyzer extracts into
   each entity's metadata. Both ship seeded with sensible defaults.
5. **Jobs** — **Run now** on `sync-repositories` for a full-fleet sweep (clone +
   analyse every selected repo). Analysis runs whenever an AI profile exists
   (the one pinned in the job config, else the default profile); with no profile
   anywhere it clones only.
6. **Platform** — browse the catalog: the **Graph** (applications by default,
   other kinds via the legend), filterable tables per entity kind, and tabbed
   per-application detail pages. Use the global **Ask** box to query the catalog
   in natural language.
7. **C4 / Dashboard / Campaigns** — view C4 architecture diagrams, the insights
   dashboard, and batch-change campaigns from the platform tabs.

Per-application actions on the detail page include **Sync** (scope a
`sync-repositories` run to just that repo), **Ask the LLM**, **LLM Hints**,
**Collect metrics**, the **File Explorer**, and the **AI Agent** tab for
change tasks. Every list/table view carries a **↻ Refresh** button (a shared
`PI.refreshButton` helper in `assets/app.js`).

## Project layout

See [`CLAUDE.md`](CLAUDE.md) for the module map and conventions, and
[`docs/`](docs/) for the milestone specifications.
