# Milestone 40 — Semantic search & duplicate detection

## Goal

Generate **embeddings** over catalog entities (applications, components, use
cases, libraries) and their AI-written summaries to power **semantic search**
("find apps that send email"), **similarity** ("apps like this one"), and
**duplicate-functionality detection** (clusters of apps/components doing the same
thing). This surfaces overlap and consolidation opportunities a keyword search and
the connection graph can't.

## Scope

- An `EmbeddingProvider` behind a trait (so it's mockable and swappable).
- An embeddings store with nearest-neighbour search, **dual-engine** (pgvector on
  Postgres; a bounded brute-force cosine fallback on SQLite).
- Generation wired into sync (or a dedicated job) for changed entities only.
- Search / similar / duplicate-cluster APIs and UI.

## Deliverables

### Embedding provider

```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
}
```

An implementation over the existing `HttpClient` (consistent with the Anthropic
provider) with a configurable model and dimension; mocked in unit tests. The text
embedded per entity is a compact, deterministic summary (name + kind + key
properties + AI description) so embeddings are stable across syncs unless the
summary changes.

### Storage & search (dual-engine)

```sql
-- migrate:up
CREATE TABLE entity_embeddings (
    entity_type TEXT NOT NULL,      -- application | component | use_case | library
    entity_id   UUID NOT NULL,
    model       TEXT NOT NULL,
    dim         INT  NOT NULL,
    vector      BYTEA NOT NULL,     -- pgvector column on PG; raw f32 blob on SQLite
    summary_hash TEXT NOT NULL,     -- skip re-embedding unchanged summaries
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (entity_type, entity_id, model)
);
-- migrate:down
```

A dual-engine `EmbeddingRepository` with `nearest(query_vec, type?, k)`:

- **Postgres:** a `vector` column + ivfflat/HNSW index for true ANN search.
- **SQLite:** store the raw `f32` blob and compute cosine in Rust over a bounded
  candidate set (the catalog is small enough; document the cap and `truncated`
  flag, consistent with other bounded features).

`summary_hash` lets generation **skip unchanged** entities (cost control; pairs
with M41 incremental).

### Generation

A `generate-embeddings` job (per-repo/per-entity, recorded for token usage so M39
prices it) that embeds new/changed entities after a sync — or a step appended to
`sync-repositories`. Only entities whose `summary_hash` changed are re-embedded.

### APIs & UI

- `GET /api/platform/search?q=...&type=` — semantic search; returns ranked entities
  with score; falls back to substring search when embeddings are absent.
- `GET /api/platform/applications/:id/similar` — nearest neighbours of an app.
- `GET /api/platform/duplicates?type=&threshold=` — clusters above a similarity
  threshold (greedy/union-find over pairwise similarity).
- UI: a global **semantic search** box, a **"Similar apps"** panel on the
  application detail, and a **"Possible duplicates"** Insights panel.

## Tasks

- [ ] `EmbeddingProvider` trait + HttpClient-backed impl + mock; configurable
      model/dim.
- [ ] `entity_embeddings` migration (pgvector on PG; blob on SQLite) + dual-engine
      `EmbeddingRepository` with `nearest` (ANN on PG, bounded cosine on SQLite).
- [ ] `generate-embeddings` job/step embedding changed entities only
      (`summary_hash`), recorded.
- [ ] Search / similar / duplicate-cluster endpoints + UI panels; substring
      fallback when no embeddings.
- [ ] Unit tests (mocked provider/repo): cosine ranking returns expected order;
      duplicate clustering groups items above threshold; unchanged summaries are
      skipped; SQLite fallback matches PG ranking on a fixed set.

## Acceptance criteria

- Catalog entities are embedded and searchable by meaning; "similar to X" and
  "possible duplicates" return sensible, ranked results.
- Search works on both engines (ANN on Postgres, bounded cosine on SQLite) and
  falls back to substring search when embeddings are unavailable.
- Generation re-embeds only changed entities; everything is unit-tested with a
  mocked embedding provider.

## Dependencies

Milestones 08/10 (entities + summaries + catalog), 13 (recorder for token cost),
26 (catalog snapshot/NL query — complementary). Cost shows up in M39; generation
benefits from M41 incremental.

## Out of scope

A full RAG chat over code, training/fine-tuning embeddings, and cross-tenant
similarity — nearest-neighbour search + duplicate clustering over catalog entities.
