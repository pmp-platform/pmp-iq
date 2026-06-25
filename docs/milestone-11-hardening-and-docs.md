# Milestone 11 — Hardening, testing & documentation

## Goal

Bring the application to a releasable state: end-to-end integration tests across
the real stack, security and operational hardening, performance checks on the
heavy paths, and complete, accurate documentation.

## Scope

- Integration/E2E test suite over a real (containerised) database.
- Security review of secrets, auth, and inputs.
- Operational concerns: config validation, graceful shutdown, observability.
- Final documentation pass (`README.md`, `CLAUDE.md`, `.env.example`).

## Deliverables

### Testing

- Integration tests (separate from unit tests) that:
  - Run migrations against a containerised Postgres and exercise the repositories.
  - Drive the HTTP API: login → configure an account + AI profile → run the
    `review-repositories` job against a fixture repo (with the AI provider
    faked at the trait boundary) → assert the platform model is populated → load
    list, detail, and graph endpoints.
- A smoke test for SQLite as the alternate driver.
- Confirm the unit-test suite remains mock-only and fast.

### Security & robustness

- Verify all secrets (repo credentials, AI keys) are encrypted at rest and never
  logged; scrub logs.
- CSRF protection on state-changing forms; session cookie flags (HttpOnly,
  Secure, SameSite); auth enforced on every non-public route.
- Input validation on all settings/job config; safe handling of untrusted repo
  content during analysis (no command injection via repo data; cloning into
  isolated workspaces).
- Rate-limit / bound concurrency on jobs, clones, and AI calls.

### Operations

- `Config::from_env()` fails fast with clear messages on invalid config.
- Graceful shutdown (drain in-flight requests/jobs).
- Structured logs with request/job correlation ids; basic metrics or counters on
  job outcomes; document `/healthz` readiness semantics.

### Documentation

- `README.md`: overview, architecture, full configuration reference, run
  instructions (`bin/up.*`, dbmate, asset refresh), feature walkthrough with
  examples (configure account → run job → explore platform).
- `CLAUDE.md`: kept minimal — current structure and essentials only.
- `.env.example`: every variable documented.

## Tasks

- [ ] Build the integration/E2E suite (containerised DB, AI faked at trait
      boundary, fixture repo).
- [ ] SQLite smoke test.
- [ ] Security pass: secrets, CSRF, cookies, validation, workspace isolation,
      rate limiting.
- [ ] Graceful shutdown + config fail-fast + correlation-id logging.
- [ ] Final `README.md`, `CLAUDE.md`, `.env.example` update.
- [ ] Verify `cargo build`, `cargo clippy` (no warnings), `cargo test` all green.

## Acceptance criteria

- The full happy path passes as an automated integration test on Postgres, and
  the SQLite smoke test passes.
- No secret appears in logs; auth, CSRF, and cookie protections are verified.
- The app starts and stops cleanly, rejects invalid config with clear errors, and
  emits correlated logs.
- Documentation lets a new operator go from clone to a populated Platform view
  using only the README.

## Dependencies

All previous milestones.

## Out of scope

New features — this milestone stabilises and documents what exists.
