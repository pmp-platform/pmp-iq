# Milestone 08 — `review-repositories` job: AI analysis & platform model

## Goal

Complete the `review-repositories` job: for each cloned repository, run the job's
selected **AI agent profile** to extract structured metadata, then persist it
into a normalised **platform model** (applications, languages, libraries,
infrastructure, app-to-app dependencies, users, groups, access grants). This is
the data foundation the Platform section (M09–M10) renders.

## Scope

- The full relational platform model + data-access traits.
- A `RepositoryAnalyzer` that turns a checkout into a typed `AnalysisResult`
  using the M05 `AiProvider`.
- A persistence/upsert layer that maps `AnalysisResult` into the model with
  stable de-duplication.
- Wiring the analysis stage into the M07 job.

## Deliverables

### Platform data model

Each table is its own dbmate migration. Suggested schema (Postgres):

```sql
-- applications: one per analysed repository (or per app within a monorepo)
CREATE TABLE applications (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    repository_id    UUID REFERENCES repositories(id) ON DELETE SET NULL,
    name             TEXT NOT NULL,
    app_type         TEXT,                  -- 'api'|'frontend'|'mobile'|'cli'|'library'|'service'|...
    description      TEXT,
    primary_language TEXT,
    metadata         JSONB NOT NULL DEFAULT '{}',
    last_analyzed_at TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- languages (deduped) + per-app usage
CREATE TABLE languages (id UUID PRIMARY KEY DEFAULT gen_random_uuid(), name TEXT UNIQUE NOT NULL);
CREATE TABLE application_languages (
    application_id UUID REFERENCES applications(id) ON DELETE CASCADE,
    language_id    UUID REFERENCES languages(id) ON DELETE CASCADE,
    percentage     NUMERIC,
    PRIMARY KEY (application_id, language_id)
);

-- libraries + versions + per-app usage
CREATE TABLE libraries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL, ecosystem TEXT NOT NULL,   -- 'cargo'|'npm'|'pip'|'maven'|...
    UNIQUE (name, ecosystem)
);
CREATE TABLE library_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    library_id UUID REFERENCES libraries(id) ON DELETE CASCADE,
    version TEXT NOT NULL,
    UNIQUE (library_id, version)
);
CREATE TABLE application_libraries (
    application_id     UUID REFERENCES applications(id) ON DELETE CASCADE,
    library_version_id UUID REFERENCES library_versions(id) ON DELETE CASCADE,
    scope              TEXT,                 -- 'runtime'|'dev'|'build'|'test'
    PRIMARY KEY (application_id, library_version_id)
);

-- infrastructure (postgres/redis/kafka/...) + per-app usage
CREATE TABLE infrastructure (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL, kind TEXT NOT NULL,        -- 'database'|'cache'|'queue'|'storage'|...
    version TEXT,
    UNIQUE (name, kind, version)
);
CREATE TABLE application_infrastructure (
    application_id    UUID REFERENCES applications(id) ON DELETE CASCADE,
    infrastructure_id UUID REFERENCES infrastructure(id) ON DELETE CASCADE,
    usage             TEXT,                 -- free text / 'primary-store' etc.
    PRIMARY KEY (application_id, infrastructure_id)
);

-- application-to-application dependencies (graph edges)
CREATE TABLE application_dependencies (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_app_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    target_app_id UUID REFERENCES applications(id) ON DELETE CASCADE,
    target_name   TEXT,                     -- when the target isn't analysed yet
    kind          TEXT,                     -- 'http'|'grpc'|'queue'|'db'|'event'
    description   TEXT,
    UNIQUE (source_app_id, COALESCE(target_app_id, '00000000-0000-0000-0000-000000000000'::uuid), target_name, kind)
);

-- discovered access principals (distinct from the auth/operator login)
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    external_id TEXT, username TEXT NOT NULL, email TEXT,
    UNIQUE (username)
);
CREATE TABLE groups (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(), name TEXT UNIQUE NOT NULL
);
CREATE TABLE group_memberships (
    group_id UUID REFERENCES groups(id) ON DELETE CASCADE,
    user_id  UUID REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);
CREATE TABLE access_grants (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    principal_type TEXT NOT NULL,           -- 'user' | 'group'
    principal_id   UUID NOT NULL,
    access_level   TEXT NOT NULL,           -- 'read' | 'write' | 'admin'
    UNIQUE (application_id, principal_type, principal_id, access_level)
);
```

> Note: `users`/`groups` here model **who can access discovered applications**,
> not who logs into Platform Inspector (that's the M03 operator). Keep them
> clearly separated in code and docs.

### Analyzer

- `RepositoryAnalyzer` trait:
  `analyze(input: AnalysisInput) -> Result<AnalysisResult, AnalysisError>` where
  `AnalysisInput` bundles the checkout path, repo metadata, and the chosen
  `AiProvider` (a struct).
- The implementation:
  - Gathers lightweight signals from the checkout via the `FileSystem` trait —
    kept small and reused. The recognised manifest files (the `SIGNAL_FILES`
    constant in `src/platform/analyzer.rs`) are: `Cargo.toml`, `package.json`,
    `requirements.txt`, `pyproject.toml`, `go.mod`, `pom.xml`, `build.gradle`,
    `Gemfile`, `composer.json`, `docker-compose.yml`, `docker-compose.yaml`,
    `Dockerfile`, `CODEOWNERS`, and `README.md`. If none are present the AI is
    told the repository has no recognised manifests. Add a filename to that
    constant to support a new ecosystem.
  - Builds a prompt instructing the AI to return **strict JSON** matching the
    `AnalysisResult` schema (app type, languages, libraries+versions,
    infrastructure, app dependencies, users/groups + access). Consult the
    `claude-api` reference for tool-use/structured-output options.
  - Calls `AiProvider::complete`, parses and validates the JSON into
    `AnalysisResult`. Invalid output triggers a bounded retry, then a recorded
    failure for that repo.

### Persistence / upsert

- A `PlatformWriter` maps `AnalysisResult` into the model with idempotent
  upserts and de-duplication (find-or-create for languages, libraries, versions,
  infrastructure, users, groups). Re-analysing a repo updates rather than
  duplicates. Each entity-type upsert is its own small function (≤ 50 lines).

### Job wiring

- Extend the `review-repositories` job: after cloning (M07), for each repo run
  the analyzer with the job's AI profile and persist via `PlatformWriter`; set
  `applications.last_analyzed_at` and `repositories.last_reviewed_at`. Per-repo
  isolation; summary counts (apps, libraries, infra, edges) in the `JobOutcome`.

## Tasks

- [ ] Migrations for every platform-model table above (one per table/group).
- [ ] Data-access traits + sqlx impls + mocks for each aggregate.
- [ ] `AnalysisResult` typed schema + JSON validation.
- [ ] `RepositoryAnalyzer` trait + impl (signals via `FileSystem`, AI via
      `AiProvider`).
- [ ] `PlatformWriter` with find-or-create upserts and dedup.
- [ ] Wire the analysis stage into the job; set review timestamps; summary stats.
- [ ] Unit tests: analyzer with a mocked `AiProvider` returning canned JSON
      (valid + invalid), and writer upsert/dedup with mocked repositories.

## Acceptance criteria

- Running `review-repositories` end to end populates applications, languages,
  libraries (+versions), infrastructure, app-to-app dependencies, and
  users/groups/access from real cloned repos.
- Re-running updates existing rows without creating duplicates.
- Malformed AI output for one repo is retried then recorded as a per-repo
  failure without aborting the run.
- Analyzer and writer are fully unit-tested with mocked AI and storage — no
  network, no real DB.

## Dependencies

Milestones 05 (AI providers), 07 (cloning + repositories table).

## Out of scope

Visualising the model (M09 tables, M10 graph).
