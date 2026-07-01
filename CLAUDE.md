# pmp-iq — project notes

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
  metadata keys outside the configured property set. **Configurable prompts
  (M34)**: `prompt_repository` (dual-engine `extraction_prompts`, stores
  overrides only — defaults live in code) + `platform::prompts` (`PromptConfig`
  per-section templates, `compose_system_prompt` joins enabled sections + always
  injects `{{json_schema}}`/`{{kinds}}`/`{{properties}}`; `validate_section`
  enforces required placeholders). `AnalysisConfig.prompts` carries them;
  `AnalysisConfigService::load()` merges overrides over defaults +
  `save_prompt`/`reset_prompt`/`metrics_prompt`. The analyzer's
  `build_system_prompt` delegates to `compose_system_prompt`; the metrics job
  prepends the configurable `metrics` section to each pass. Settings tabs "Entity
  kinds" / "Properties" / "Extraction prompts"; routes in
  `routes/analysis_config.rs` (kinds/properties + `GET/PUT
  /api/settings/extraction-prompts[/:section]`, `POST .../:section/reset`). Every
  entity table carries a `metadata` JSONB/TEXT column.
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
  = fetch + hard-reset the working tree to `origin/<branch>`). Credential callback
  is `allowed`-type aware: HTTPS uses the token (userpass), SSH uses the agent
  then a default on-disk key (so a user's `url.*.insteadOf` SSH rewrite still
  authenticates); `to_https_clone_url` rewrites SSH remotes to HTTPS when a token
  is present.
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
- `src/campaigns/` — **batch-change campaigns** (M30): a named change applied
  across many repositories. `model`/`repository` (dual-engine `campaigns`); a
  campaign drives one multi-repo agent task (M23) — selection by explicit
  `application_ids` or an allowlisted applications filter (blank = whole fleet),
  resolved to repos and fanned out (one PR per repo). Routes `GET/POST
  /api/platform/campaigns`, `GET /api/platform/campaigns/:id` (per-repo progress
  from the task targets); page `/platform/campaigns` (Campaigns tab).
- `src/c4.rs` — **C4 model** (M29 + M38): projects the platform graph
  (applications = systems; infra/services/external = external systems; edges =
  relationships) into Structurizr DSL + C4 Mermaid at three levels.
  **Context** (`structurizr_dsl`/`mermaid_context`) takes an
  `include_dependencies` flag — **applications only by default** (keeping app→app
  relationships), `?dependencies=true` adds the external systems. **Container**
  (`container_dsl`/`container_mermaid`, M38) projects a focused app graph into its
  datastores (`ContainerDb` via `is_datastore` keyword match) + services +
  external boundaries. **Component** (`component_dsl`/`component_mermaid`, M38)
  projects an app's `detail()` into components, inter-component edges (shared
  use cases) and component→external edges (dependency `component_id`). Page
  `/platform/c4` (level selector + app picker, "Include dependencies" checkbox) +
  `GET /api/platform/c4?level=context|container|component&application=&dependencies=`
  → `{dsl, mermaid}`; `assets/platform-c4.js`.
- `src/techradar/` — **version currency & tech radar** (M45): `currency` (pure
  `parse_semver`/`major_behind`/`eol_status`/`assess`/`currency_score`),
  dual-engine `TechRadarRepository` (`version_policy` seeded by
  `ensure_default_policies` + `tech_radar`). Routes
  `GET /api/platform/applications/:id/currency` (assess libraries vs policy),
  `/api/platform/currency` (fleet, least-current first),
  `GET/POST /api/platform/tech-radar`, `DELETE .../:id` (admin); page
  `/platform/tech-radar` (Tech radar tab); `assets/tech-radar.js` (radar +
  fleet + app-detail dependency-currency panel).
- `src/codebase_map.rs` — **codebase map** (M28): `build_map` derives a
  bounded directory/module structure graph (nodes = directories, edges =
  containment; depth/node caps with `truncated`) from a cloned checkout via the
  sandboxed `FileBrowser`. Route `GET /api/platform/applications/:id/codebase-map`;
  "Codebase Map" G6 tab on app detail, rendered as a left-to-right `compact-box`
  tree (rect nodes). (Import-dependency edges are future work.)
- `src/dashboard.rs` — **insights dashboard** (M32): pure `build(apps, metrics)`
  aggregates latest metrics + the application list into rollups, leaderboards
  (top/needs coverage, lowest/highest complexity), and group-by (coverage by
  type/language). Page `/platform/dashboard` (Insights tab) + `GET
  /api/platform/dashboard`; `assets/platform-dashboard.js`. **Trends & charts
  (M35)**: `metrics/series` (pure `daily_average`, `histogram`,
  `allowed_dimension`) + `ApplicationMetricsRepository`
  `history`/`history_by_dimension`/`app_history`. Routes
  `GET /api/platform/series?metric=&dimension=&window=`,
  `/distribution?metric=&buckets=`, `/portfolio`,
  `/applications/:id/series` (`routes/trends.rs`). Charts drawn with the
  self-contained vendored `assets/vendor/minichart.js` (SVG line/histogram/
  scatter/treemap/sparkline; no CDN): dashboard trend lines + distribution +
  scatter + treemap (`assets/platform-trends.js`), per-app sparklines on the
  app detail.
- `src/metrics/` — **quality metrics** (M31, expanded M33): `model`
  (`Metric`/`ApplicationMetric` — carries a `category`), `registry`
  (`MetricCategory` + `category_for(key)`: the single source of truth mapping each
  key to a theme, stamped onto every row at write time), dual-engine
  `ApplicationMetricsRepository` (`record`/`latest_for_application`/`latest_all`;
  history kept; writes `category` via the registry), `job` (`CollectMetricsJob`,
  type `collect-metrics`: per-repo-locked clone → multiple LLM `PASSES` over the
  checkout — **code-health** (tests/coverage/complexity/LOC/CI + duplication/lint/
  todo/doc/convention-compliance) and **security** (vuln counts/outdated deps/
  secrets/…) — parsed generically by `parse_fields` (omit nulls), plus **derived**
  metrics (`source=derived`, no LLM) computed by `derived_metrics` from
  `platform.detail` (architecture fan-out/external + model-coverage component/
  use-case/observability/diagram presence)). Routes
  `GET`/`POST /api/platform/applications/:id/metrics`; `GET` also returns
  `collecting` and `POST` is **deduped** — it won't enqueue a second collection
  while one is queued/running for the same application (returns the in-flight one
  with `already_running:true`), via `JobExecutionRepository::active_for_job`.
  Insights tab on app detail groups metrics by category and disables Collect while
  one is in progress.
- `src/cost/` — **LLM cost & token budgeting** (M39): `model`
  (`LlmUsageInput`/`Budget`/`BudgetScope` (global/profile/job/application)/
  `BudgetPeriod` (daily/monthly) with `start`/`next_start`), `pricing` (pure
  `cost()` + `PriceTable` — built-in Claude prices, exact→prefix→default model
  match, `with_overrides` from `config.pricing`; `price_rows` aggregates
  per-(key,model) into sorted `CostRow`s), `repository` (dual-engine
  `LlmUsageRepository`: `record`/`usage_since(scope)`/`grouped(dimension)`/
  `usage_for_execution`; `LlmBudgetRepository` CRUD), `guard` (`BudgetGuard` —
  sums period-to-date cost per applicable scope → `BudgetDecision` Ok/Warn/Stop).
  The `RecordingAiProvider` appends a priced `llm_usage` row per call via
  `with_usage(repo, UsageAttribution)` (wired in the review/metrics/agent/llm
  jobs); `jobs::enforce_budget` translates a `Stop` into `JobError::CannotRun`
  (review + metrics jobs pre-flight). Routes `GET /api/platform/cost` (Insights
  cost panel: month/day spend, projection, top spenders, budget status),
  `GET /api/jobs/executions/:id/cost`, `GET/POST /api/cost/budgets`,
  `DELETE /api/cost/budgets/:id`; `assets/platform-cost.js` on the dashboard.
- `src/embeddings/` — **semantic search & duplicate detection** (M40): `model`
  (pure `build_summary`/`summary_hash`/`cosine`/`rank`/`cluster` (union-find) +
  f32 blob `to_blob`/`from_blob`), `provider` (`EmbeddingProvider` trait +
  `HttpEmbeddingProvider` over `HttpClient`, OpenAI/Voyage-compatible
  `/embeddings`), `repository` (dual-engine `entity_embeddings`: `upsert`/
  `hashes`/`all`/`nearest` — bounded cosine scan in Rust, both engines;
  `neighbours_of`), `job` (`GenerateEmbeddingsJob`, type `generate-embeddings`:
  embeds only entities whose `summary_hash` changed; sources via
  `PlatformQuery::embedding_sources` = applications/components/use_cases).
  Provider built from `config.embedding` (env `EMBEDDING_ENDPOINT/MODEL/API_KEY`);
  job registered + seeded only when configured. Routes `GET /api/platform/search`
  (semantic → substring fallback), `/applications/:id/similar`,
  `/api/platform/duplicates?type=&threshold=`; `assets/semantic.js` (search box
  on graph, "Similar applications" on app detail, "Possible duplicates" on
  dashboard).
- `src/platform/api_endpoints.rs` — **API contracts** (M42): macro-based dual-engine
  `ApiEndpointRepository` (`for_application`/`consumers`) over `api_endpoints` +
  `endpoint_files`. The analyzer extracts `endpoints` (configurable section, M34;
  protocol ∈ http/grpc/graphql, others dropped) and a dependency's `endpoint`
  operation; the writer upserts endpoints (delete-and-recreate, partial-write
  aware M41) and resolves each consumer dependency to a producer endpoint
  (`application_dependencies.endpoint_id`). Route
  `GET /api/platform/applications/:id/endpoints` (endpoints + per-endpoint
  consumers = impact); `assets/platform-api.js` API panel on the app detail.
- `src/platform/changes.rs` + `src/audit/` — **diff / timeline & audit** (M36).
  `changes`: `Change`/`ChangeKind` + pure `diff_keys`/`application_change`/
  `summarize`, dual-engine `PlatformChangeRepository` (`platform_changes`:
  `record`/`timeline`/`between`). The `PlatformWriter` snapshots prior app
  fields + dependency keys before its delete-and-recreate and emits precise
  application created/updated + dependency created/removed change rows.
  `audit`: dual-engine `AuditRepository` (`audit_events`) + `AuditService`
  (best-effort `record`), called from login + prompt edits. Routes
  `GET /api/platform/timeline`, `/applications/:id/timeline`,
  `/api/platform/diff?from=&to=&application=`, `GET /api/audit` (admin); page
  `/platform/audit` (Audit tab, admin-gated); `assets/timeline.js` (audit table +
  global feed + per-app "Recent changes" panel).
- `src/scorecards/` — **production-readiness scorecards** (M43): `engine` (pure
  `evaluate(input, checks) -> Scorecard` over built-in rules — `has_owner`,
  `metric_min`/`metric_max`, `has_observability`/`has_diagrams`/`documented`;
  weighted score + `level_for` bronze/silver/gold/at_risk capped by a failed
  `critical` check; `default_checks`), dual-engine `ScorecardRepository`
  (`scorecard_checks` seeded by `ensure_default_checks` at boot + `scorecard_results`).
  Routes `GET /api/platform/applications/:id/scorecard` (compute + record) and
  `/api/platform/scorecards` (fleet, ranked) in `routes/scorecards.rs`; inputs =
  `platform.detail` + latest metrics (M31) + `rbac.owner_team_names` (M37);
  `assets/platform-scorecard.js` (app panel + dashboard fleet).
- `src/gamification/` — **operator gamification** (M44): `engine` (pure
  `award_for(action)` points+skill map, `level_for` curve, `badges_earned`),
  dual-engine `GamificationRepository` (`xp_awards` idempotent by
  `(actor,reason,source)` + `badges`), `service` (`GamificationService.replay`
  replays audit events → XP+badges idempotently; `profile`/`leaderboard`), `job`
  (`GamificationJob` hourly cron, seeded in `main`). Mutating routes (team/role,
  prompt, agent-task, campaign, budget) audit their action (M36) so it's
  awardable. Routes `GET /api/gamification/me`/`leaderboard`,
  `POST .../replay` (admin); page `/platform/leaderboard` (Leaderboard tab);
  `assets/gamification.js`.
- `src/remediation/` — **auto-remediation** (M46): `trigger` (pure
  `trigger_matches(trigger_kind, params, &AppSignals)` — `metric_below`/
  `metric_above`/`scorecard_failed`/`dep_eol`; a missing metric never matches),
  dual-engine `RemediationRepository` (`remediation_rules` + `remediations`,
  `propose` deduped per `(rule, finding_key)` via `ON CONFLICT DO NOTHING`),
  `service` (`RemediationService.evaluate(&[(app, AppSignals)])` proposes for
  matching rules respecting each rule's `scope.application_ids`; `mark_running`).
  Routes (`routes/remediation.rs`): `GET/POST /api/platform/remediation/rules`,
  `DELETE .../:id` (admin); `POST /api/platform/remediation/evaluate` (builds
  per-app `AppSignals` from metrics + scorecard fails + EOL deps, then proposes);
  `GET /api/platform/remediations?status=`; `POST /api/platform/remediations/:id/
  approve` (opens an agent task for the app — `can_mutate_app`-gated — and marks
  the remediation `running`) / `dismiss`. Page `/platform/remediation`
  (Remediation tab); `assets/remediation.js`.
- `src/dora/` — **DORA metrics** (M47): `compute` (pure `compute(deployments,
  incidents, window_days) → DoraSummary` — deploy frequency, lead time,
  change-failure rate, MTTR + a composite elite/high/medium/low tier from the
  worst sub-band), `model` (`Deployment`/`Incident`/`NewDeployment`/`DoraSummary`),
  dual-engine `DoraRepository` (`deployments` + `incidents`; record/open/resolve +
  windowed reads per-app and fleet). Routes (`routes/dora.rs`):
  `POST /api/events/{deploy,incident}` + `.../incident/:id/resolve` (generic
  authenticated ingestion; deploy resolves app via `application_id` or
  `repository_full_name`), `GET /api/platform/applications/:id/dora` (records the
  summary as `dora_*` metrics — `delivery` category — so it trends via M35) and
  `GET /api/platform/dora` (fleet + per-app rows). Webhooks (M25) ingest GitHub
  `deployment_status` (success/failure) + `release`. DORA panel on the Insights
  dashboard + per-app detail panel; `assets/platform-dora.js`.
- `src/rbac/` — **roles, teams & multi-tenant** (M37): `model` (`Role`
  viewer<maintainer<admin, `highest`; `Team`), dual-engine `RoleRepository`
  (`roles`) + `TeamRepository` (`teams`/`team_members`/`team_applications`),
  `service` (`RbacService`: `role_for` — assignment wins, else bootstrap admin /
  empty-table → admin, else viewer; `can_mutate_app` — admin any, maintainer only
  team-owned; `visible_app_ids` — tenant scope when `config.multitenant`),
  `middleware` (`role_guard` via `from_fn_with_state(Role, …)`). Role is enriched
  onto the session `Principal` at login. Admin routes `GET/POST /api/teams`,
  `DELETE /api/teams/:id`, `/api/teams/:id/members|applications`,
  `GET/POST /api/roles` (`routes/rbac.rs`, role_guard-gated); `sync_application`
  + application detail/list are ownership/tenant-scoped. Settings "Teams & roles"
  tab (`assets/teams.js`).
- `src/nl_query.rs` — **Ask the platform** (M26): `CatalogQuery` answers a
  natural-language question grounded in a serialised `GraphQuery` snapshot of the
  catalog (system prompt forbids inventing data). Route `POST /api/platform/ask`;
  global ask box on the graph page (`assets/platform-ask.js`).
- `src/pr_watcher.rs` — **PR watcher** (job type `pr-watcher`, cron `* * * * *`):
  polls `list_open_pr_targets`; finishes merged/closed PRs, and on new review
  comments / merge conflicts / failed checks posts a marker comment (`🤖 pmp-iq:`,
  used to dedup) and enqueues a continue-branch agent fix turn as a **queued**
  execution that the M27 dispatcher runs. Provider gains `pull_request_status`/
  `pull_request_comments`/`pull_request_checks`/`post_pull_request_comment`
  (GitHub impl; `Unsupported` defaults) + `AccountService` wrappers. The agent job
  gained `continue_branch` (resume the PR branch vs. fresh from default).
- `src/llm_request/` — `LlmRepositoryRequestJob` (job type `llm-repository-request`):
  clones/fetch-rebases a repo and runs an LLM session over the checkout, serialised
  by a per-repository lock; records full I/O + tokens; answer in execution metadata.
  `ensure_job` seeds the singleton job that backs application Q&A.
- `src/repositories/` — cloned-repo records: `RepoRecord` (incl.
  `last_analyzed_sha`, M41), `RepoRecordRepository` (`mark_analyzed`).
- `src/incremental.rs` — **incremental analysis** decision (M41): pure
  `is_structural`/`requires_full` (manifests/lockfiles/CI/CODEOWNERS force full),
  `decide(last_sha, base_missing, changed) → Mode::{Full,Incremental}`, and
  `affected` (invert file attribution → affected entity names). `GitClient`
  gains `changed_files(checkout, from, to) → ChangedFiles{paths, base_missing}`
  (git2 diff). The review job's `analyze` calls `plan_mode` (incremental only
  when the job param `incremental` is set, a prior analyzed commit exists, HEAD
  advanced, base reachable, no structural change), passes `AnalysisInput.
  changed_files` (analyzer adds an `incremental_focus` prompt), writes via
  `PlatformWriter::write_partial` (upsert only the affected components/use cases,
  preserve untouched, no prune), and records `mark_analyzed(head)`. Webhook
  scoped re-syncs pass `incremental:true`.
- `src/review/` — `ReviewRepositoriesJob` (job type `sync-repositories`):
  clones selected repos from enabled accounts, then (when an AI profile is
  available) analyses each and writes the platform model. `build_provider` uses
  `resolve_profile_id`: the profile pinned in the job config `ai_profile_id`, else
  the default profile (`AiProfileService::default_profile_id`) — so a sync seeded
  without a profile still analyses once one exists; clone-only only when none is
  configured anywhere. Snapshots the
  entity catalog once at run start (`platform.catalog()`) and runs
  `catalog::resolve_dependencies` on each result before write to canonicalize
  dependency targets. After the sweep it calls `writer.prune_orphans()` to delete
  shared entities no longer referenced. `ensure_sync_job` pre-seeds the singleton
  job at boot (in `main`, like the other `ensure_job`s), wiring the default AI
  profile; it backfills `ai_profile_id` into an existing job that lacks one when a
  profile becomes available. Per-app sync passes a `repository_id` param to scope
  the sweep; an empty param = full-fleet sweep.
- `src/platform/` — `analysis` (`AnalysisResult` schema + parse/validate;
  `LinkedInfo` backs every linked entity; dependencies are code-derived outbound
  connections, each carrying a `component` name the writer resolves to a
  `components` row (`application_dependencies.component_id`);
  `AnalysisConfig`/`KindDef`/`PropertyDef`
  + `apply_config` — drop disallowed kinds, strip unconfigured metadata keys),
  `analyzer`
  (`RepositoryAnalyzer`/`FileAnalyzer` — manifest signals + AI; system prompt
  built from `AnalysisConfig` passed via `AnalysisInput`; sets `working_dir` to
  the checkout so the Claude CLI reads real files, and `retain_existing_files`
  drops component/use-case `files` paths absent from the checkout so the file
  viewer never 404s), `linked`
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
  emits both per use case), conditional per-relation tables, an **Interactions**
  tab (outbound `dependencies` — http/db/queue/… — each with a "Details" button
  opening the implementing component's files), and an always-present Members tab.
  Diagrams have zoom/reset controls; wheel-zoom off.
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
  `members_for`). An account carries an optional `organization`: for All/regex
  selection the GitHub/GitLab providers filter the token's visible repos to that
  namespace (`in_namespace`/`scope_to_namespace`, subgroups included); for List
  selection `select_for` resolves each entry directly via
  `RepositoryProvider::get_repository` (`GET /repos/{owner}/{repo}`, GitLab
  `/projects/{enc}`; bare names org-prefixed) so a token's outside-collaborator
  repos are found even when a listing omits them. Default `get_repository` scans
  the listing (Local).
- `src/routes/` — axum routers, merged in `routes::router` (public vs
  `require_auth`-gated). Sessions via tower-sessions `MemoryStore`. `AppState`
  is built via `AppState::build` (validates `ENCRYPTION_KEY`). `routes/webhooks.rs`
  (M25, public, HMAC-verified `POST /webhooks/github`): PR events trigger an
  immediate `pr-watcher` reconcile; a default-branch push enqueues a scoped
  `sync-repositories` run. Secret from `webhooks.github_secret` config.
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
