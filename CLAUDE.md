# PlatIQ — project notes

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
- `src/config.rs` — layered config: optional `config.yaml` (`ConfigLoader`
  over `FileSystem`+`EnvSource`; binary-adjacent default, `--config-file` flag,
  `${VAR}`/`${VAR:-default}` interpolation) with precedence env > file > default.
  `RedisConfig`, `AuthProvider`, `log_level`; env-only `Config::load(EnvSource)`.
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
  `SecretGenerator` traits, `AuthService` (ordered strategies; admin default,
  GitHub appended when `auth.provider=github`), session `require_auth` middleware.
  `github` (`GitHubIdentity` trait + `HttpGitHubIdentity` over `HttpClient`;
  `GitHubLoginStrategy` for the personal-token form path; `authorize` org/login
  allowlist). OAuth web flow (`/auth/github/login` + `/auth/github/callback`) in
  `routes/auth.rs` using `AppState::github_auth`.
- `src/web/` — `TemplateEngine` (embedded minijinja) + `render_page`.
- `src/crypto.rs` — `Encryptor` trait + `AesGcmEncryptor` (secrets at rest).
- `src/httpclient.rs` — `HttpClient` trait + `ReqwestClient`, `ThrottledHttpClient`
  (min-interval throttle), `HttpResponse` (with headers). Git providers throttle
  via this and map 429 / rate-limit headers to `ProviderError::RateLimited`
  (→ `AppError::RateLimited`).
- `src/fs.rs` — `FileSystem` trait (`list_subdirs`/`list_files`/`read_to_string`/
  `create_dir_all`/`exists`) + `RealFileSystem`.
- `src/files/` — `FileBrowser` (lazy one-level tree + size-capped file reads over
  `FileSystem`) + `safe_join` (path-traversal guard) for the File Explorer.
- `src/process.rs` — `CommandRunner` trait + `TokioCommandRunner` (`CommandSpec`
  carries an optional `cwd`).
- `src/ai/` — AI agent profiles: `provider` (`AiProvider` trait), `anthropic`
  (Messages API over `HttpClient`), `claude_cli` (over `CommandRunner`; runs in
  `AiRequest.working_dir` so it can read a checkout), `factory`
  (`AiProviderFactory`/`AiProviderDeps`), `repository`, `service`
  (`AiProfileService`). Default model `claude-opus-4-8`.
- `src/locks/` — `DistributedLock` trait (named lock + TTL + refresh): `InMemoryLock`
  (default), SQL-backed `Pg/SqliteSqlLock` over `controller_locks`, and `RedisLock`
  (over a mockable `RedisClient`; `SET NX PX` + token-checked Lua refresh/release,
  selected when `redis.enabled`); `lock_keys` helpers. Backend chosen in
  `store::distributed_lock`. Used for controller leader election + per-job/per-repo
  serialisation.
- `src/git.rs` — `GitClient` trait + `Git2Client` (`clone_or_update`; `sync_branch`
  = fetch + hard-reset the working tree to `origin/<branch>`).
- `src/workspace.rs` — `Workspace` (per-job dirs `{root}/jobs/{name}/{id}` via
  `JobLocator`, persisted across runs) over `FileSystem`.
- `src/hints/` — per-entity LLM hints: `model`, dual-engine `EntityHintRepository`
  (keyed by `application_id`+`entity_type`+natural `entity_key`; survive re-sync),
  `render_hints` (prompt block injected into analysis as authoritative corrections).
- `src/agent_tasks/` — application **AI Agent** change tasks (job type
  `application-agent-task`): `model` (`AgentTask`/`AgentTaskMessage`/
  `AgentTaskTarget` + statuses), dual-engine `AgentTaskRepository` (tasks +
  transcript + per-repo **targets**), `job` (`AgentTaskJob` — per-target,
  per-repo-locked turn: sync default branch → `agent/<id>` branch → agentic Claude
  CLI over the checkout → `commit_all` → `push_branch` → open/update a PR via
  `AccountService::open_pull_request`; updates target+task status; no-change →
  `awaiting_input`). **Multi-repo (M23)**: a task has many `agent_task_targets`
  (one branch/PR/status each); turns fan out one job execution per target (params
  carry `target_id`). Routes: single `POST /api/platform/applications/:id/
  agent-tasks`, app-agnostic multi `POST /api/platform/agent-tasks`
  (`application_ids`), follow-up messages enqueue a turn per target. UI tab
  `assets/platform-app-agent.js`. `GitClient` gains
  `create_branch`/`commit_all`/`push_branch`; `RepositoryProvider` gains
  `open_pull_request`/`get_pull_request` (GitHub impl, `Unsupported` default).
- `src/llm_request/` — `LlmRepositoryRequestJob` (job type `llm-repository-request`):
  clones/fetch-rebases a repo and runs an LLM session over the checkout, serialised
  by a per-repository lock; records full I/O + tokens; answer in execution metadata.
  `ensure_job` seeds the singleton job that backs application Q&A.
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
  The platform graph shows applications only by default (other kinds opt-in via
  the legend); the application detail is client-side **tabbed** (`assets/
  platform-app-detail.js`): Overview (focused app→deps→infra G6 graph +
  properties/languages), Use cases (G6 flowchart of use cases → click opens a
  wide modal with Sequence + Component mermaid diagrams; the analyzer always
  emits both per use case), conditional per-relation tables, and an
  always-present Members tab. Diagrams have zoom/reset controls; wheel-zoom off.
- `src/jobs/` — jobs subsystem: `model` (`ExecStatus` incl. `Skipped`;
  `JobError::Failed|CannotRun{retry_at}`; `merge_object` JSON helper),
  `repository` (jobs + executions; `list_due`/`set_next_run_at`,
  `merge_metadata`, params on `create`; `heartbeat`/`list_stale`/`cancel`),
  `job_type` (`JobType` trait + `JobTypeRegistry`; `JobContext` carries
  `job_id`/`job_name`/`params`/`clock` and exposes `append_output`/
  `merge_metadata`/`recording_provider`/`save_state`/`pause_requested`/
  `heartbeat`/`heartbeat_guard`; `HeartbeatGuard` background ticker), `runner`
  (`JobRunner` — status lifecycle, pause/resume, `start_with_params`, reschedule
  on `CannotRun` → `Skipped`; holds a background `HeartbeatGuard` for every
  execution so a long-but-healthy job is never cancelled mid-run; guards terminal
  marking so a stale-cancelled run isn't overwritten; `ExecutionRun`), `recording`
  (`RecordingAiProvider` — mirrors
  LLM I/O to the execution output + token usage to metadata), `scheduler`
  (`CronScheduler`), `controller` (`JobController` — leader-elected loop, via
  `DistributedLock`, that resumes due paused executions, starts jobs whose
  `next_run_at` elapsed, and `cancel_stale`s running executions whose
  `heartbeat_at` is >5 min old), `builtin` (`NoopJob`). Jobs carry `next_run_at`;
  executions carry `state`/`resume_at`/`pause_requested`/`params`/`metadata`/
  `heartbeat_at`. The runner heartbeats each execution in the background for its
  whole run, so staleness only catches executions whose worker has died (its
  heartbeat task can no longer beat). Job types register in `AppState::build`;
  the controller is spawned in `main`. Jobs carry a `max_concurrency` (in
  `config.max_concurrency`, default 1); the runner admits up to that many running
  executions per job and **queues** the rest (no more blanket 409), and the
  controller's `dispatch_queued` starts queued executions as slots free (global
  cap `GLOBAL_MAX_ACTIVE`). The agent-task job's concurrency comes from
  `agent.max_concurrency` config (default 4).
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
- Docker topologies: `bin/up.sh single` (one app, SQLite) or `bin/up.sh
  distributed` (app1+app2+nginx+Postgres+Redis). `Dockerfile` (multi-stage),
  `docker-compose.{single,distributed}.yml`, `deploy/{single,distributed}/`.
