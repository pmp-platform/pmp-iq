# Milestone 25 — Webhooks: PR events & merge-driven re-sync

## Goal

Add **webhook-driven** triggers as the lower-latency, more scalable alternative
to the M24 poller. The app receives provider webhooks for:

1. **Pull-request events** (review comments, check runs, PR closed/merged) — and
   drives the **same** finish/fix logic as M24 via the shared `PrReconciler`; and
2. **Push / merge events on a default branch** (a main merge) — and enqueues a
   `sync-repositories` run scoped to that repository, keeping the platform model
   fresh as code lands.

Webhooks and the M24 poller are complementary: webhooks react in near-real-time
and avoid constant polling, but require a publicly reachable endpoint and per-repo
registration. Both feed one reconciliation core; running both is safe (deduped).

## Scope

- Verified webhook endpoints for GitHub (and GitLab) PR + push events.
- PR events → the M24 `PrReconciler` (no logic duplication).
- Default-branch push/merge → scoped `sync-repositories` enqueue.
- Signature verification, event dedup/replay protection, and clear "verify and
  enqueue fast, do work async" handling.

## Deliverables

### Endpoint & verification

A public route module `src/routes/webhooks.rs` (mounted **outside**
`require_auth`, like `health`):

- `POST /webhooks/github` (and `/webhooks/gitlab`).
- **Signature verification**: GitHub `X-Hub-Signature-256` HMAC-SHA256 over the
  raw body with a configured secret (GitLab `X-Gitlab-Token`). Reject unverified
  with `401`; the secret comes from `config.yaml` (M18) — add a `webhooks`
  section (`github_secret`, `gitlab_secret`, both `${VAR}`-interpolable).
- **Dedup**: remember recent delivery ids (`X-GitHub-Delivery`) to ignore
  retries/replays (small TTL set, e.g. in the existing `controller_locks`/a tiny
  table or an in-memory LRU per instance).
- **Fast ack**: verify + parse + enqueue, then return `2xx` immediately; the
  heavy work runs through jobs (never block the webhook response).

### PR events → reconciliation

Map the relevant events to a task target and run the shared core:

- Events: `pull_request` (closed/merged/synchronize), `pull_request_review`,
  `pull_request_review_comment`, `issue_comment` (on a PR), `check_run` /
  `check_suite` (completed).
- Resolve the PR (repo + number / head branch) to an `agent_task_targets` row;
  if none matches (a PR the agent didn't open), ignore.
- Invoke `PrReconciler` (M24) for that target — finishing the task on
  merged/closed, or enqueuing an agent fix-turn on a new comment / conflict /
  failed check. Because the reconciler is idempotent and high-water-marked, a
  webhook and the poller acting on the same event converge without double-fixing.

### Merge-driven re-sync

- On `push` (GitHub) / `Push Hook` (GitLab) to a repository's **default branch**,
  resolve the repository record and enqueue a `sync-repositories` run **scoped to
  that repository** (reuse `review::ensure_sync_job` + `start_with_params`,
  mirroring `routes/platform.rs::sync_application`).
- Non-default-branch pushes are ignored. This replaces "wait for the next
  scheduled sync" with "re-analyse on merge".

### Registration & docs

- Document registering the webhook on the provider (URL + secret + events), or
  auto-registering it via the **GitHub App** when M21's app credentials are
  configured (create-hook API, best-effort, idempotent).
- README "Webhooks vs polling" guidance: enable webhooks for low latency/scale;
  the M24 poller remains the zero-config default and a safety net.

## Tasks

- [ ] `webhooks` config section (secrets, `${VAR}`) on `Config` (M18).
- [ ] `routes/webhooks.rs`: GitHub/GitLab endpoints, signature verification,
      delivery-id dedup, fast-ack.
- [ ] PR-event mapping → `PrReconciler` (M24) for the matched target.
- [ ] Default-branch push → scoped `sync-repositories` enqueue.
- [ ] Optional GitHub App auto-registration of the webhook.
- [ ] Unit tests (mocked verifier/reconciler/runner): a valid signed PR event
      reaches the reconciler; an invalid signature is `401`; a duplicate delivery
      id is ignored; a default-branch push enqueues a scoped sync; a non-default
      push is ignored.

## Acceptance criteria

- A signed PR webhook drives the same finish/fix behaviour as the M24 poller,
  with near-real-time latency and without polling.
- A merge to a repository's default branch triggers a scoped re-sync of its
  platform model.
- Unverified or duplicate deliveries are rejected/ignored; the endpoint always
  acks fast and does work asynchronously.
- The reconciliation logic is shared with M24 (no duplication) and unit-tested
  with mocked dependencies.

## Dependencies

Milestones 24 (`PrReconciler`, provider PR introspection), 22/23 (tasks +
targets), 18 (config for secrets), 07/08 (`sync-repositories`), 21 (GitHub App
credentials for auto-registration).

## Out of scope

Guaranteed exactly-once delivery (dedup is best-effort), provider event types
beyond PR + push, and a webhook management UI — registration is documented/CLI or
GitHub-App-driven.
