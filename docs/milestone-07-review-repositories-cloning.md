# Milestone 07 — `review-repositories` job: cloning

## Goal

Implement the first half of the `review-repositories` job type: resolve the
selected repositories across all configured accounts and **clone (or update)**
them into local working copies, recording each repository in the database. This
is the input stage for the AI analysis in M08.

## Scope

- `repositories` table tracking discovered/cloned repos.
- A `GitClient` trait + implementation for clone/fetch/checkout.
- The `review-repositories` `JobType` wired into M06's runner — cloning stage
  only.
- Reuse of M04's providers and `RepoSelector`.
- Workspace management and concurrency/cleanup.

## Deliverables

### Data model

```sql
-- migrate:up
CREATE TABLE repositories (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id       UUID NOT NULL REFERENCES repository_accounts(id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    full_name        TEXT NOT NULL,
    clone_url        TEXT NOT NULL,
    default_branch   TEXT,
    local_path       TEXT,                  -- workspace checkout path
    last_cloned_at   TIMESTAMPTZ,
    last_commit_sha  TEXT,
    last_reviewed_at TIMESTAMPTZ,           -- set by M08
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, full_name)
);

-- migrate:down
```

### Git access

- `GitClient` trait:
  - `clone_or_update(target: CloneRequest) -> Result<CheckoutInfo, GitError>`
    where `CloneRequest` bundles clone URL, destination path, branch, and auth
    (a struct), and `CheckoutInfo` returns the resolved commit SHA + path.
- Implementation via `git2` (or the system `git` behind a `CommandRunner`).
  Network/process access is behind the trait so the job is unit-testable.
- Credentials for private repos come from the account (decrypted via the M04
  `Encryptor`).

### Job: cloning stage

- The `review-repositories` `JobType`:
  1. Load enabled accounts from config/selection on the job.
  2. For each account, build its `RepositoryProvider`, `list_repositories()`,
     apply `RepoSelector` (all/regex/list).
  3. Upsert each selected repo into `repositories`.
  4. `clone_or_update` into a per-job workspace; update `last_cloned_at`,
     `last_commit_sha`, `local_path`.
  5. Emit progress to the `LogSink`; record counts in the `JobOutcome` summary.
- Bounded concurrency for clones; partial failures recorded per repo without
  aborting the whole run.

### Workspace

- A configurable workspace root; a `Workspace` helper allocates per-job
  directories and cleans them up (behind a `FileSystem` trait for testability).

## Tasks

- [ ] Migration for `repositories`.
- [ ] `RepositoryRepository` trait + sqlx impl + mock.
- [ ] `GitClient` trait + impl; `CloneRequest`/`CheckoutInfo`.
- [ ] `Workspace` helper over a `FileSystem` trait.
- [ ] `review-repositories` job: account → provider → selector → upsert → clone.
- [ ] Per-repo error isolation + summary counts.
- [ ] Unit tests: selection→upsert→clone flow with mocked provider, git client,
      filesystem, and repositories — no network or disk.

## Acceptance criteria

- Running the job clones the selected repositories from each configured account
  into the workspace and records them in `repositories` with commit SHAs.
- A repo that fails to clone is recorded as failed but doesn't fail the whole
  run; re-running updates existing checkouts (fetch, not re-clone).
- The full flow is unit-tested with mocked git/provider/filesystem.

## Dependencies

Milestones 04 (accounts/selector/encryptor) and 06 (jobs framework).

## Out of scope

Analysing repository contents and populating the platform model (M08).
