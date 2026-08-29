# ADR-0140: A `pgvector` recall store scoped to `projected_nodes`, not the curated `knowledge` domain

- Status: Proposed
- Date: 2026-08-29
- Deciders: MindLeak maintainers (proposed in session; awaiting repository-owner
  review per this repo's adoption convention)
- Refines: [ADR-0136](0136-ackplane-gains-an-mcp-front-door-not-a-duplicated-storage-core.md)
  (decision 3 named this as one of three follow-up gaps; this ADR resolves
  the recall-store question, the last of the three)
- Depends on: [ADR-0008](0008-semantic-recall-embedding-index.md) (the local
  `recall` contract this must match: embeddings seed graph traversal, they
  never replace it), [ADR-0087](0087-the-ackplane-graph-is-a-projection-not-an-authority.md)
  (`projected_nodes`/`projected_edges` are a rebuildable ledger projection —
  the property this new table must preserve), [ADR-0113](0113-the-industrial-knowledge-plane-is-evidence-backed-and-human-governed.md)
  (why the existing `knowledge_embeddings` table is the wrong fit for this)
- Related: [ADR-0002](0002-sqlite-decay-over-vector-llm.md) (decay over
  vector-only memory — the same governing principle this store must not
  regress)

## Context

ADR-0136 decision 3 named a `pgvector`-backed recall store as the third open
follow-up, without specifying its shape: "add a `pgvector` embeddings table
scoped to `projected_nodes` itself (parallel to, but distinct from, the
curated `knowledge_embeddings` table), so `ackplane-mcp`'s `recall` has a real
graph-wide backing store instead of reusing a domain scoped for a different
purpose."

Read the actual schemas and the local `recall` implementation directly rather
than assuming a bare vector index would suffice. `crates/ackplane-server/
migrations/0007_knowledge.sql` already uses `pgvector` — `vector(768)`, the
nomic-embed-text dimension the local planes also default to — but its
`knowledge`/`knowledge_embeddings` tables back a categorically different
domain: [ADR-0113](0113-the-industrial-knowledge-plane-is-evidence-backed-and-human-governed.md)
requires that domain's contents to be evidence-backed and human-governed —
curated, confirmed statements with a `retired_at`/`retired_reason`/
`retired_by` lifecycle. `projected_nodes` (`0002_projection.sql`) is the
opposite kind of thing: every git commit, symbol, execution, and tool
invocation the ledger has ever recorded, decaying continuously, never
curated. Backing graph-wide `recall` with the curated table would either
flood it with uncurated noise (defeating ADR-0113's governance) or silently
narrow "recall across the whole graph" down to "recall across whatever a
human already confirmed" (defeating what `recall` means locally). Neither is
an acceptable reuse; a distinct table is the only correct shape, exactly as
ADR-0136 decision 3 already anticipated.

The more consequential finding is in `crates/mindleak-core/src/embed.rs`
itself. Local `recall` is not a bare "rank by cosine similarity" query. It is
a deliberately layered discrimination pipeline, and the file's own comments
document *why*, with a dated measurement: a raw cosine floor cannot separate
a real hit from background noise, because "embedding spaces are anisotropic,
so every text carries a baseline resemblance to every other text" — measured
directly against this repository's own index, the nonsense query `zzzzqqq
wibble flarp` scored 0.54, *above* the naive 0.5 floor, because the whole
field scores around that for any query. Three mechanisms correct for this,
all three load-bearing:

1. **`kind_prior`** — a per-`NodeType` ranking weight (Intent/Digest highest,
   Agent lowest) reflecting "what a question is for," applied as a
   tie-breaker over raw cosine, never an override, so a genuinely closer
   symbol still outranks a barely-related intent.
2. **`distinctive_cut`** — a per-query statistical test: a candidate must
   stand out from its own query's field by `DISTINCTIVE_SIGMA` (1.0) standard
   deviations above the field's mean, not merely clear an absolute floor. A
   field too small to have a shape (`DISTINCTIVE_MIN_FIELD`, 8) falls back to
   the floor alone; a field with zero spread (every candidate resembles the
   query equally, the nonsense-query shape) returns nothing at all.
3. **The floor** — ADR-0053's original contract that recall must be able to
   answer nothing rather than always answering, preserved underneath the
   above two refinements rather than replaced by them.

A `pgvector` `ORDER BY embedding <=> query LIMIT n` — the obvious naive
port — implements none of this. It would ship a **regression** relative to
today's SQLite-backed `recall`, not parity: exactly the failure mode
`embed.rs`'s own comment already measured and fixed locally ("a caller handed
a plausible stranger cannot tell it is wrong, and stops asking"). This ADR
exists specifically to stop that regression from being the accidental
default once a "real" vector index becomes available and looks sufficient on
its own.

## Decision

**A new `projected_node_embeddings` table extends the existing
`projected_nodes`/`projected_edges` projection with `pgvector`-backed
embeddings, populated only for nodes that already exist in the projection,
rebuildable under the same ledger discipline as its siblings. `ackplane-mcp`'s
`recall` tool ranks against it using the same three-mechanism discrimination
pipeline `mindleak-core::embed::recall` already implements locally — kind
prior, distinctive-field cut, and floor — not a bare `pgvector` distance
`ORDER BY`.**

1. **Schema.** `projected_node_embeddings (tenant_id, repository_id, node_id,
   model, embedding vector(768), updated_at, PRIMARY KEY (tenant_id,
   repository_id, node_id, model), FOREIGN KEY (tenant_id, repository_id,
   node_id) REFERENCES projected_nodes ON DELETE CASCADE)` — the same shape
   `knowledge_embeddings` already uses (decoupled by `(node_id, model)` so a
   node may be re-embedded under a new model without losing history, degrading
   to structural results rather than erroring for a model it was never
   embedded under), applied to the projection instead of the curated domain.
   An `ivfflat` index on `embedding vector_cosine_ops` supports the distance
   query the ranking pipeline below still needs as its first stage.

2. **Population follows the projection's own rebuild discipline, not a
   parallel ingestion path.** `Projector::rebuild` already replays a
   repository's accepted `structural_fact` ledger records into
   `projected_nodes`/`projected_edges` (ADR-0087). Embedding computation is a
   second, optional pass over that same projected set — the same
   two-phase shape `mindleak-core` already uses locally (index nodes, then
   embed the ones missing an embedding, `nodes_missing_embeddings`) — never a
   second writer inventing nodes the ledger replay did not produce. A node
   present in `projected_nodes` with no corresponding
   `projected_node_embeddings` row is `not_yet_embedded`, a legible state, not
   a silent gap indistinguishable from "recall found nothing here."

3. **Ranking is a two-stage pipeline, not a single query.** Stage one asks
   PostgreSQL for a bounded candidate set via `pgvector`'s `<=>` operator
   (this is where `pgvector` earns its place: candidate retrieval at scale,
   the exact SQLite scaling limit `0007_knowledge.sql`'s own comment already
   names — "not pulled into application memory for a cosine loop over a
   BLOB"). Stage two applies `kind_prior` and `distinctive_cut` against that
   candidate set's cosine scores, in application code structurally identical
   to `mindleak-core::embed`'s existing implementation — ideally the *same*
   code, factored into a small shared crate (see decision 5), not a
   second, independently-written copy that can silently drift from the
   constants and reasoning the local version's comments already justify with
   a dated measurement.

4. **The reported score stays raw cosine; the floor and distinctiveness cut
   still gate whether anything is reported at all.** A caller sees the exact
   similarity that was measured, exactly as local `recall` already promises,
   and an unanswerable query still returns nothing rather than a plausible
   stranger — the same ADR-0053 guarantee, not a weaker one for the
   Postgres-backed profile.

5. **The discrimination constants and logic are a genuine reuse candidate for
   the shared crate ADR-0136 decision 6 already permits.** `kind_prior`,
   `distinctive_cut`, `DISTINCTIVE_MIN_FIELD`, and `DISTINCTIVE_SIGMA` are pure
   functions over `NodeType` and `f32` scores — no `rusqlite`, no `tokio-postgres`,
   nothing storage-specific. Factoring them out (alongside `mindleak-core::decay`,
   already named in ADR-0136 decision 6) is the second, not the first, genuinely
   shared piece of logic between the two profiles' storage layers — both
   candidates identified only because their contracts already matched, not
   because unification was a goal in itself.

6. **This does not change what `recall` means locally.** `mindleak-mcp`'s own
   `recall` and `embed.rs` are untouched. This ADR adds a second, Postgres-side
   implementation of the same discrimination contract for the Industrial
   profile; it does not migrate, wrap, or proxy the local one.

## Consequences

- `ackplane-mcp`'s `recall` tool becomes genuinely useful rather than either
  unavailable or a quiet regression — it answers over the graph-wide,
  ledger-derived projection, with the same "stand out or say nothing"
  guarantee the local planes already rely on.
- Ackplane gains a second `pgvector`-backed table, structurally close to
  `knowledge_embeddings` but serving a deliberately different, uncurated,
  decay-shaped domain — consistent with, not a violation of, ADR-0113's
  narrower governance scope for the curated table.
- A future divergence risk is named explicitly: if `kind_prior`/
  `distinctive_cut` are reimplemented independently rather than shared, the
  two profiles' recall behavior can silently drift as the local version's
  constants are tuned. Decision 5's shared-crate reuse is how this ADR closes
  that risk rather than merely noting it.
- Embedding computation itself (the model call) stays off any hot path,
  consistent with this repository's zero-token-ingestion invariant — this ADR
  changes storage and ranking, not when or whether a model is called.

## Rejected alternatives

**Rank purely by `pgvector`'s `<=>` distance, no discrimination pipeline.**
Rejected: measured directly against this repository's own local index, this
degrades below today's SQLite-backed behavior — a nonsense query scores above
a naive floor, and a caller cannot tell a returned result is noise. This is
the specific regression this ADR exists to prevent.

**Reuse the existing `knowledge`/`knowledge_embeddings` tables for graph-wide
recall.** Rejected: ADR-0113 requires that domain's contents to be
evidence-backed and human-governed. Populating it from every ledger-derived
node would either flood it with uncurated content (violating ADR-0113) or
require filtering `recall` down to only human-confirmed knowledge (silently
narrowing what graph-wide recall means, unlike the local planes' `recall`).

**Compute the discrimination pipeline in PostgreSQL as a stored function
instead of application code.** Considered, not rejected outright — a `plpgsql`
port of `kind_prior`/`distinctive_cut` is plausible future work if profiling
shows the candidate-set round-trip is a bottleneck. Deferred here because it
would fork the *logic*, not just the *storage*, of a calculation this ADR's
decision 5 specifically wants kept as one shared, tested implementation;
moving it into SQL trades that guarantee for a performance gain not yet shown
to be needed.
