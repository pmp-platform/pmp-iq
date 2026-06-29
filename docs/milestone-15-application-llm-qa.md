# Milestone 15 — Application Q&A ("Ask the LLM about this application")

## Goal

On the application detail page, add a prompt box at the top that lets the user
ask anything about the project. Submitting a question enqueues an
**`llm-repository-request`** execution (M14) for that application's repository
with the question as input, and the page polls and renders the answer when the
job completes.

## Scope

- An "Ask" endpoint that enqueues an `llm-repository-request` execution scoped to
  the application's repository.
- A status/result endpoint the page polls for the answer.
- A prompt box + answer area pinned at the top of the application detail page.

## Deliverables

### Enqueue endpoint

`POST /platform/applications/{id}/ask` (under `require_auth`), body
`{ "question": "..." }`:

- Resolves the application → its repository (`applications.repository_id` →
  `repositories` full name + default branch).
- Selects the AI profile: a configured default (a `default_ai_profile_id` app
  setting) or the first enabled profile; return a clear error when none exists.
- Enqueues against the built-in `llm-repository-request` job via
  `JobRunner::start(job_id, "manual", params)` with
  `params = { repository, branch, input: question, ai_profile_id }` (M13
  per-execution params, M14 job).
- Returns the new `execution_id`.

### Result endpoint

`GET /platform/applications/{id}/ask/{execution_id}` (or reuse the existing
job-execution read path) returning `{ status, output, metadata, answer }` so the
page can show progress and the final answer (the answer comes from
`metadata.answer`, M14).

### UI

In `templates/platform_app_detail.html` + `assets/platform-app-detail.js`:

- A prompt `textarea` + "Ask" button pinned at the top of the detail page,
  visible across the tabs.
- On submit: POST the question, then jQuery-poll the result endpoint (reuse the
  polling pattern in `assets/job-detail.js`) until the execution reaches a
  terminal state.
- Render the answer (markdown or plain text), show token usage from `metadata`,
  and link to the full job execution detail (raw LLM input/output).
- Show pending/failed states clearly; a question that is rescheduled because the
  repo lock is held (M14) shows a "queued — waiting for the repository" state.
- A short history of prior questions for this application (executions of the job
  scoped to its repository) is optional.

## Tasks

- [ ] `POST /platform/applications/{id}/ask` enqueues the job with per-execution
      params; resolves repository + AI profile.
- [ ] Result endpoint exposing status/output/metadata/answer.
- [ ] Prompt box + answer area + polling in the app-detail page (reuse
      `job-detail.js` polling).
- [ ] Unit tests: ask handler resolves repository + profile and enqueues correct
      params (mocked runner/queries); no-profile and unknown-app error paths.

## Acceptance criteria

- Asking a question on an application page shows a pending state, then the LLM's
  answer, with token usage and a link to the raw job execution.
- The answer reflects the repository's contents (the job ran the LLM over the
  checkout).
- Concurrent questions for the same repository queue behind the per-repo lock
  rather than running in parallel.
- Handler logic is unit-tested with mocked dependencies.

## Dependencies

Milestones 09 (application detail page), 13 (per-execution params), 14
(`llm-repository-request`).

## Out of scope

Multi-turn chat threads (this is single-shot Q&A), and the LLM hints / file
features (M16–M17).
