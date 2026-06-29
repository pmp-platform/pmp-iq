# Milestone 26 — Natural-language query over the whole catalog

## Goal

A global **"Ask the platform"** interface: the user asks a question in natural
language about the **entire** platform model — applications, languages,
libraries, infrastructure, dependencies, users/groups, use cases, components —
and gets an answer **grounded in the catalog**, with links to the entities it
used. Where M15 answers questions about one application's repository, this answers
questions across the whole model ("which applications depend on Kafka and are
owned by team X?", "what infrastructure does the payments domain use?", "which
libraries are on an end-of-life version?").

## Scope

- A bounded **tool-calling agent** that answers by querying the existing
  `PlatformQuery` read layer through an allowlisted set of read tools — never by
  emitting raw SQL and never by inventing data.
- A global chat/search entry point that renders a cited answer linking to entity
  detail pages, plus the underlying tool calls/results for transparency.
- Reuse of the AI provider + recording infrastructure.

## Deliverables

### Read tools over the catalog

The agent is given a small, **allowlisted** toolset that wraps `PlatformQuery`
(no arbitrary queries; each tool is paginated and filter-validated like the
existing `ListQuery`/`filter_fields`):

- `find_entities(entity_type, search?, filters?, page?)` — paginated list of any
  catalog entity type with the same allowlisted filters as the tables UI.
- `get_application(id)` / `get_entity(entity_type, id)` — a detail view
  (properties, languages, libraries, dependencies, members…).
- `dependencies_of(application_id)` / `dependents_of(target)` — traverse the
  dependency graph (reuse `GraphQuery`/catalog name-join) for impact-style
  questions.
- `facets(entity_type)` — the allowed filter values (so the agent can ground
  filter terms like a library/kind name).

Each tool returns compact JSON with stable entity ids/names the answer can cite.
The tools are **read-only** and reuse the engine-dispatched query layer, so they
work identically on Postgres and SQLite.

### The query agent

A `catalog_query` capability (a service, optionally backed by a job for long
runs):

- Builds an `AiRequest` with a system prompt describing the available tools and
  the catalog schema, then runs a **bounded tool-use loop** (the Anthropic /
  Claude CLI provider's tool calling): the model calls `find_entities` /
  `get_*` / `dependencies_of` / `facets`, the service executes them against
  `PlatformQuery`, feeds results back, and iterates until the model answers.
- Caps the number of tool calls and total tokens; streams progress; records the
  full tool transcript + token usage via the M13 recorder.
- Returns a synthesised answer plus the **entities referenced** (ids/types/names)
  so the UI can render links and the user can verify the grounding.
- An optional semantic layer (future): embed entity `name`/`description` so the
  agent can resolve fuzzy terms to entities before filtering — additive, not
  required for the structured path.

### UI

- A top-level **"Ask the platform"** box (in the platform header / its own tab),
  separate from the per-application ask (M15).
- On submit: call the query agent (sync with a bounded loop, or enqueue + poll
  like M15), render the answer as Markdown with **inline links** to the cited
  entity detail pages, and a collapsible "how this was answered" panel showing
  the tool calls + results (transparency, and a debugging aid).
- Empty/zero-result and "couldn't ground this" states are explicit (the agent
  must say when the catalog doesn't contain the answer rather than guessing).

## Tasks

- [ ] Allowlisted read tools over `PlatformQuery`/`GraphQuery`
      (`find_entities`/`get_*`/`dependencies_of`/`dependents_of`/`facets`),
      paginated + filter-validated.
- [ ] `catalog_query` agent: bounded tool-use loop, caps, recording, referenced-
      entity output.
- [ ] Global "Ask the platform" UI: answer with entity links + tool transcript.
- [ ] Unit tests (mocked AI provider + mocked `PlatformQuery`): the agent calls
      the tools and grounds its answer; tool filters are allowlisted (an
      unlisted filter is rejected); a no-data question yields an explicit
      "not found" rather than a fabricated answer; the tool-call cap is enforced.

## Acceptance criteria

- A natural-language question about the whole catalog is answered from the model
  via the read tools, with links to the entities used and a visible tool
  transcript.
- The agent cannot emit arbitrary queries — it only calls the allowlisted,
  filter-validated tools — and says so when the catalog lacks the answer.
- Works on both database engines (the tools go through the existing query layer);
  the agent and tools are unit-tested with mocked AI + query dependencies.

## Dependencies

Milestones 09 (`PlatformQuery` lists/filters/detail/facets), 10
(`GraphQuery`/catalog traversal), 05 (AI providers), 15 (ask UI/polling pattern
to reuse).

## Out of scope

A write/agentic path from the chat (this is read-only Q&A; changes go through
M22–M23), text-to-SQL against the raw database, and cross-tenant access controls
(single-tenant, admin-only, consistent with the rest of the app).
