# pmp-iq — Milestones

pmp-iq is a Rust 2024 web application that connects to one or more
source-control accounts (GitHub, GitLab, or local repositories), clones the
selected repositories, runs AI-driven analysis over them, and builds a queryable
**platform model**: applications, the languages/libraries they use, the
infrastructure they depend on, how they connect to each other, and which
users/groups can access them. The result is browsable as connection graphs and
filterable tables.

These documents describe the project as an ordered set of milestones. Each
milestone is independently shippable, builds on the previous ones, and ends with
explicit acceptance criteria. Implement them in order.

## Milestone index

| #  | File | Title |
|----|------|-------|
| 00 | [milestone-00-foundation.md](milestone-00-foundation.md) | Project foundation & tooling |
| 01 | [milestone-01-database-and-migrations.md](milestone-01-database-and-migrations.md) | Database layer & migrations (dbmate) |
| 02 | [milestone-02-http-and-static-assets.md](milestone-02-http-and-static-assets.md) | HTTP API foundation & local static assets |
| 03 | [milestone-03-authentication.md](milestone-03-authentication.md) | Authentication & login strategies |
| 04 | [milestone-04-settings-repository-accounts.md](milestone-04-settings-repository-accounts.md) | Settings — repository accounts |
| 05 | [milestone-05-settings-ai-agent-profiles.md](milestone-05-settings-ai-agent-profiles.md) | Settings — AI agent profiles |
| 06 | [milestone-06-jobs-infrastructure.md](milestone-06-jobs-infrastructure.md) | Jobs infrastructure & scheduler |
| 07 | [milestone-07-review-repositories-cloning.md](milestone-07-review-repositories-cloning.md) | `review-repositories` job — cloning |
| 08 | [milestone-08-review-repositories-analysis.md](milestone-08-review-repositories-analysis.md) | `review-repositories` job — AI analysis & platform model |
| 09 | [milestone-09-platform-tables.md](milestone-09-platform-tables.md) | Platform section — tables, filters & detail pages |
| 10 | [milestone-10-platform-graph.md](milestone-10-platform-graph.md) | Platform section — connection graph |
| 11 | [milestone-11-hardening-and-docs.md](milestone-11-hardening-and-docs.md) | Hardening, testing & documentation |
| 12 | [milestone-12-distributed-locks.md](milestone-12-distributed-locks.md) | Distributed locks abstraction |
| 13 | [milestone-13-job-scheduling-workspaces-and-live-updates.md](milestone-13-job-scheduling-workspaces-and-live-updates.md) | Job scheduling, per-job workspaces & live updates |
| 14 | [milestone-14-llm-repository-request-job.md](milestone-14-llm-repository-request-job.md) | `llm-repository-request` job type |
| 15 | [milestone-15-application-llm-qa.md](milestone-15-application-llm-qa.md) | Application Q&A — ask the LLM |
| 16 | [milestone-16-llm-hints-for-application-entities.md](milestone-16-llm-hints-for-application-entities.md) | LLM hints for application entities |
| 17 | [milestone-17-file-attribution-and-explorer.md](milestone-17-file-attribution-and-explorer.md) | Use-case/component file attribution & File Explorer |
| 18 | [milestone-18-config-file.md](milestone-18-config-file.md) | Optional `config.yaml` configuration file |
| 19 | [milestone-19-redis-distributed-lock.md](milestone-19-redis-distributed-lock.md) | Redis distributed-lock backend |
| 20 | [milestone-20-docker-compose-topologies.md](milestone-20-docker-compose-topologies.md) | Docker Compose topologies (single & distributed) |
| 21 | [milestone-21-github-login.md](milestone-21-github-login.md) | GitHub login (GitHub App / personal token) |
| 22 | [milestone-22-application-ai-agent-tasks.md](milestone-22-application-ai-agent-tasks.md) | Application "AI Agent" tab — change tasks & PRs |
| 23 | [milestone-23-multi-repo-agent-tasks.md](milestone-23-multi-repo-agent-tasks.md) | Multi-repository AI Agent tasks |
| 24 | [milestone-24-pr-watcher-polling.md](milestone-24-pr-watcher-polling.md) | PR watcher (polling) — comments, conflicts & failed checks |
| 25 | [milestone-25-webhooks.md](milestone-25-webhooks.md) | Webhooks — PR events & merge-driven re-sync |
| 26 | [milestone-26-catalog-nl-query.md](milestone-26-catalog-nl-query.md) | Natural-language query over the whole catalog |
| 27 | [milestone-27-configurable-job-concurrency.md](milestone-27-configurable-job-concurrency.md) | Configurable job concurrency |
| 28 | [milestone-28-codebase-maps.md](milestone-28-codebase-maps.md) | Auto-generated interactive codebase maps |
| 29 | [milestone-29-c4-model.md](milestone-29-c4-model.md) | C4 model views & export |
| 30 | [milestone-30-batch-changes.md](milestone-30-batch-changes.md) | Batch changes — large-scale edits across many repos |
| 31 | [milestone-31-quality-metrics.md](milestone-31-quality-metrics.md) | LLM-collected quality metrics (tests, coverage, complexity) |
| 32 | [milestone-32-platform-dashboard.md](milestone-32-platform-dashboard.md) | Platform metrics & insights dashboard |
| 33 | [milestone-33-expanded-metrics.md](milestone-33-expanded-metrics.md) | Expanded metrics catalog (LLM-sourced) |
| 34 | [milestone-34-configurable-extraction-prompts.md](milestone-34-configurable-extraction-prompts.md) | Configurable extraction prompts (per section, in Settings) |
| 35 | [milestone-35-metric-trends-and-charts.md](milestone-35-metric-trends-and-charts.md) | Metric trends & charts |
| 36 | [milestone-36-platform-diff-timeline-audit.md](milestone-36-platform-diff-timeline-audit.md) | Platform diff / timeline & audit |
| 37 | [milestone-37-rbac-teams-multitenant.md](milestone-37-rbac-teams-multitenant.md) | RBAC, teams & multi-tenant |
| 38 | [milestone-38-c4-container-component-levels.md](milestone-38-c4-container-component-levels.md) | C4 Container & Component levels |
| 39 | [milestone-39-llm-cost-and-token-budgeting.md](milestone-39-llm-cost-and-token-budgeting.md) | LLM cost & token budgeting |
| 40 | [milestone-40-semantic-search-and-duplicate-detection.md](milestone-40-semantic-search-and-duplicate-detection.md) | Semantic search & duplicate detection |
| 41 | [milestone-41-incremental-analysis.md](milestone-41-incremental-analysis.md) | Incremental analysis |
| 42 | [milestone-42-api-contracts.md](milestone-42-api-contracts.md) | API contracts & endpoint-level dependencies |
| 43 | [milestone-43-production-readiness-scorecards.md](milestone-43-production-readiness-scorecards.md) | Production-readiness scorecards |
| 44 | [milestone-44-user-gamification.md](milestone-44-user-gamification.md) | Operator gamification (levels, XP, skills, badges) |
| 45 | [milestone-45-version-currency-tech-radar.md](milestone-45-version-currency-tech-radar.md) | Version currency & tech radar |
| 46 | [milestone-46-auto-remediation-tasks.md](milestone-46-auto-remediation-tasks.md) | Auto-remediation tasks |
| 47 | [milestone-47-dora-metrics.md](milestone-47-dora-metrics.md) | DORA metrics |

## Architecture at a glance

- **Edition / language:** Rust 2024.
- **HTTP framework:** `axum` (Tokio async runtime).
- **Database:** PostgreSQL (default) or SQLite, accessed through `sqlx`. All data
  access goes through repository **traits** so the concrete database is an
  implementation detail and can be mocked in unit tests.
- **Migrations:** `dbmate` (`amacneil/dbmate:2.28.0`). Up-only migrations,
  `--no-dump-schema`.
- **UI:** Server-rendered HTML (templating engine) enhanced with **jQuery** and
  styled with **Tailwind CSS**. All third-party JS/CSS assets are downloaded and
  served locally — no CDNs at runtime.
- **Auth:** Pluggable login strategies. The first strategy is a single admin user
  from `ADMIN_USER` / `ADMIN_PASS`, or an auto-generated `admin` account at boot.
- **Repository providers:** Strategy pattern — GitHub, GitLab, and local
  repositories, each behind a common trait.
- **AI providers:** Strategy pattern — Anthropic API and the Claude CLI binary,
  each behind a common trait.

## Cross-cutting engineering standards

These apply to **every** milestone and are not repeated in each file:

- Introduce a **trait/interface for every external dependency** (database, HTTP
  clients, git, AI providers, clock, filesystem) so they can be mocked in unit
  tests.
- **No unit test may touch a real external service** (database, cache, network,
  filesystem, env). Mock behind the relevant trait.
- Functions stay **under 50 lines**; files stay well **under 1000 lines**; keep
  modules small and focused.
- A function takes **at most four parameters** — use a parameter struct beyond
  that — and returns **at most two values** (prefer `Result<T, E>`); wrap richer
  returns in a struct.
- Repeated logic (string helpers, formatting, etc.) is **extracted into shared,
  reused utilities**.
- `main.rs` is an **entrypoint only**: load configuration, build services and
  repositories, start the server. No business logic.
- **dbmate** migrations are **up-only** (include the `-- migrate:down`
  delimiter but leave it empty) and run with `--no-dump-schema`.
- Provide `bin/up.sh` + `bin/up.bat` and `bin/down.sh` + `bin/down.bat` wrapping
  `docker compose`, both accepting an optional profile argument.
- A feature is **not done** while any test fails or the build is broken. Never
  skip or weaken a test to make it pass — fix the underlying cause.
- Keep `README.md` and `CLAUDE.md` updated as features land. `CLAUDE.md` stays
  minimal (structure + essentials only).

## Definition of done (per milestone)

1. Code compiles with `cargo build` and passes `cargo clippy` with no warnings.
2. `cargo test` passes; new logic has unit tests with mocked dependencies.
3. Database changes ship as dbmate migrations that apply cleanly.
4. `README.md` documents the new feature; `CLAUDE.md` reflects any structural
   change.
5. The milestone's acceptance criteria are demonstrably met.
