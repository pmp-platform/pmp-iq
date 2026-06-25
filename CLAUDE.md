# Platform Inspector — project notes

Rust 2024 web app: connects to git accounts (GitHub/GitLab/local), clones
selected repos, runs AI analysis, and builds a queryable platform model
(applications, libraries, infrastructure, dependencies, users/groups) browsable
as tables and a connection graph.

## Stack
- axum + tokio, minijinja templates, jQuery + Tailwind (vendored in `assets/`).
- Dual database: SQLite (default, zero-config) or PostgreSQL, both via sqlx.
  `Database` is an enum over `PgPool`/`SqlitePool`; each repository trait has a
  `Pg*` and `Sqlite*` impl, picked by engine in `src/store.rs`. UUIDs generated
  in Rust. SQL authored in Postgres `$N` style and translated for SQLite via
  `db::to_sqlite`. Per-engine dbmate migrations (`db/migrations`,
  `db/migrations_sqlite`), also embedded; SQLite auto-migrates at boot
  (`db::migrate`). Postgres uses dbmate.
- Strategy patterns: repo providers (github/gitlab/local), AI providers
  (anthropic/claude_cli), login strategies.

## Layout
- `src/main.rs` — entrypoint only (config → services → serve).
- `src/config.rs` — `Config::load(EnvSource)`; env behind `EnvSource` trait.
- `src/db/` — `Database` enum (pg/sqlite pools), `RepoError`, `to_sqlite`
  placeholder translator, `migrate` (idempotent embedded-migration applier).
- `src/store.rs` — engine-dispatching factories returning `Arc<dyn Repo>` for
  each trait (Pg or SQLite impl chosen from the `Database`).
- `src/error.rs` — `AppError` + HTTP mapping.
- `src/appsettings.rs` — `SettingsRepository` (key/value).
- `src/auth/` — `LoginStrategy`/`StaticAdminStrategy`, `PasswordHasher` +
  `SecretGenerator` traits, `AuthService`, session `require_auth` middleware.
- `src/web/` — `TemplateEngine` (embedded minijinja) + `render_page`.
- `src/crypto.rs` — `Encryptor` trait + `AesGcmEncryptor` (secrets at rest).
- `src/httpclient.rs` — `HttpClient` trait + `ReqwestClient`, `ThrottledHttpClient`
  (min-interval throttle), `HttpResponse` (with headers). Git providers throttle
  via this and map 429 / rate-limit headers to `ProviderError::RateLimited`
  (→ `AppError::RateLimited`).
- `src/fs.rs` — `FileSystem` trait + `RealFileSystem`.
- `src/process.rs` — `CommandRunner` trait + `TokioCommandRunner`.
- `src/ai/` — AI agent profiles: `provider` (`AiProvider` trait), `anthropic`
  (Messages API over `HttpClient`), `claude_cli` (over `CommandRunner`),
  `factory` (`AiProviderFactory`/`AiProviderDeps`), `repository`, `service`
  (`AiProfileService`). Default model `claude-opus-4-8`.
- `src/git.rs` — `GitClient` trait + `Git2Client` (clone/fetch).
- `src/workspace.rs` — `Workspace` (per-job dirs over `FileSystem`).
- `src/repositories/` — cloned-repo records: `RepoRecord`,
  `RepoRecordRepository`.
- `src/review/` — `ReviewRepositoriesJob` (job type `review-repositories`):
  clones selected repos from enabled accounts, then (when the job config sets
  `ai_profile_id`) analyses each and writes the platform model.
- `src/platform/` — `analysis` (`AnalysisResult` schema + parse/validate),
  `analyzer` (`RepositoryAnalyzer`/`FileAnalyzer` — manifest signals + AI),
  `writer` (`PlatformWriter` — idempotent find-or-create upserts into the
  applications/languages/libraries/infrastructure/dependencies/users/groups/
  access model), `query` (`PlatformQuery` — paginated/searchable lists + detail
  views for applications/infrastructure/libraries/users/groups via `ListQuery`
  + `Page<T>`), `graph` (`GraphQuery`/`GraphScope` — nodes+edges for the
  connection graph, focus + truncation). Routes in `routes/platform.rs`;
  generic list/detail templates + cytoscape graph page.
- `src/jobs/` — jobs subsystem: `model`, `repository` (jobs + executions),
  `job_type` (`JobType` trait + `JobTypeRegistry`; `JobContext` exposes
  `state`/`save_state`/`pause_requested`), `runner` (`JobRunner` — status
  lifecycle, pause/resume; `JobOutcome::Completed|Paused`), `scheduler`
  (`CronScheduler`), `leader` (`LeaderLock` distributed lock), `controller`
  (`JobController` — leader-elected loop that resumes paused executions whose
  `resume_at` elapsed), `builtin` (`NoopJob`). Executions carry
  `state`/`resume_at`/`pause_requested`. Job types register in `AppState::build`;
  the controller is spawned in `main`.
- `src/accounts/` — repository accounts: `model`, `repository`
  (`RepositoryAccountRepository`), `providers` (github/gitlab/local +
  `RepositoryProviderFactory` over `ProviderDeps`), `selector` (`RepoSelector`),
  `service` (`AccountService`).
- `src/routes/` — axum routers, merged in `routes::router` (public vs
  `require_auth`-gated). Sessions via tower-sessions `MemoryStore`. `AppState`
  is built via `AppState::build` (validates `ENCRYPTION_KEY`).
- `src/app.rs` — `AppState` + `build_router`.
- `db/migrations/` — dbmate migrations (up-only; `--no-dump-schema`).
- `tests/common/mod.rs` — `TestDb::start()` boots a Postgres testcontainer and
  applies migrations; shared by integration tests.
- `docs/` — milestone specs.

## Conventions
- Every external dependency sits behind a trait; unit tests use mockall mocks
  and never touch real services. Integration tests use testcontainers.
- Functions < 50 lines, files well under 1000, ≤ 4 params (else a struct),
  ≤ 2 returns (else a struct / `Result`).

## Commands
- Build/test: `cargo build`, `cargo test` (integration tests need Docker).
- DB up + migrate: `bin/up.sh migrate` (profile `migrate` runs dbmate).
- New migration: `dbmate new <name>` (keep `migrate:down` empty).
