# Platform Inspector — Milestones

Platform Inspector is a Rust 2024 web application that connects to one or more
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
