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
- `src/analysis_config/` — user-configured analysis vocabulary: `model`
  (`EntityKind`/`EntityProperty` — both have id + friendly `name` + `description`;
  kinds also a free-form `config` JSON (for diagram/observability-signal kinds),
  properties also a `DataType`), `repository` (dual-engine `entity_kinds` +
  `entity_properties` stores, full CRUD), `service` (`AnalysisConfigService`:
  CRUD + `load()` → `platform::AnalysisConfig`). Allowed kinds (per entity type,
  incl. application `app_type` + library `ecosystem`) and extraction properties
  are injected into the analysis prompt strictly (the LLM must use only listed
  ids/keys); on import `AnalysisResult::apply_config` drops entities with an
  unlisted kind (an invalid application `app_type` is cleared) and strips
  metadata keys outside the configured property set. Settings tabs "Entity
  kinds" / "Properties"; routes in `routes/analysis_config.rs`. Every entity
  table carries a `metadata` JSONB/TEXT column.
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
- `src/review/` — `ReviewRepositoriesJob` (job type `sync-repositories`):
  clones selected repos from enabled accounts, then (when the job config sets
  `ai_profile_id`) analyses each and writes the platform model. Snapshots the
  entity catalog once at run start (`platform.catalog()`) and runs
  `catalog::resolve_dependencies` on each result before write to canonicalize
  dependency targets. After the sweep it calls `writer.prune_orphans()` to delete
  shared entities no longer referenced.
- `src/platform/` — `analysis` (`AnalysisResult` schema + parse/validate;
  `LinkedInfo` backs every linked entity; dependencies are code-derived outbound
  connections, each carrying a `component` name the writer resolves to a
  `components` row (`application_dependencies.component_id`);
  `AnalysisConfig`/`KindDef`/`PropertyDef`
  + `apply_config` — drop disallowed kinds, strip unconfigured metadata keys),
  `analyzer`
  (`RepositoryAnalyzer`/`FileAnalyzer` — manifest signals + AI; system prompt
  built from `AnalysisConfig` passed via `AnalysisInput`), `linked`
  (`LinkedEntity` registry: the table-driven `(name,kind,version,metadata)`
  entities — infrastructure, tools, cloud-providers, services, platforms,
  external — sharing one writer/query/graph code path), `writer`
  (`PlatformWriter` — idempotent find-or-create upserts into the
  applications/languages/libraries/linked-entities/dependencies/users/groups/
  access model + app-owned sub-entities (components, use_cases,
  use_case_components, diagrams, observability_signals — delete-and-recreate per
  sync via CASCADE); `reconcile_members` upserts provider members as `member` with
  permissions, flips departed ones to `ex_member`, while AI access is written as
  `codeowner` — `access_grants` carries `association_type` + `permissions`, one
  row per principal per app; `prune_orphans` deletes unreferenced shared entities
  (libs/versions/languages/linked), never users/groups), `query` (`PlatformQuery` — paginated/searchable/
  filterable lists + `facets` for filter dropdowns + detail views for
  applications/libraries/users/groups + every linked entity via `ListQuery`
  (`filters` allowlisted per entity via `filter_fields`) + `Page<T>` + `catalog()`
  snapshot), `catalog` (`Catalog` — canonicalizes a dependency's free-form
  `target_name` to a known entity via exact→normalized→fuzzy matching, with a
  bounded provider shortlist for ambiguous fuzzy cases; rewriting the name lights
  up the existing read-layer name-join — no schema change), `graph` (`GraphQuery`/`GraphScope` — nodes+edges for
  the connection graph, focus + truncation). Routes in `routes/platform.rs`
  (`/platform` redirects to the Graph tab); shared `_platform_tabs.html`,
  generic + application-specific detail templates, and an AntV G6 graph page.
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
  `RepositoryProviderFactory` over `ProviderDeps`; `RepositoryProvider::list_members`
  → `RepoMember`, implemented for GitHub via the collaborators API, empty default
  elsewhere), `selector` (`RepoSelector`), `service` (`AccountService` exposes
  `members_for`).
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
