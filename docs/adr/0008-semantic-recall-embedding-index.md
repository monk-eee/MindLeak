# ADR-0008 — Optional semantic recall via a local embedding index (complements, never replaces, the decay graph)

- **Status:** Accepted
- **Date:** 2026-07-22

## Context

[ADR-0002](0002-sqlite-decay-over-vector-llm.md) chose a SQLite decay graph
**over** vector-only memory, and rejected per-event LLM work on the write path.
That decision stands — and the impact-precision experiment
([`scripts/experiments/impact-vs-similarity.mjs`](../../scripts/experiments/impact-vs-similarity.mjs))
now backs it with numbers: on *"what breaks if I change X?"* the graph scored
**100% precision/recall** against lexical similarity's **25%**, because
similarity retrieves files that *look like* X while the graph retrieves files
that *actually depend on* X.

But that same experiment names the one axis where vectors legitimately win and
the graph is weak: **fuzzy semantic recall** — *"which node is about X?"* when
you do not know its id, name, or imports. FTS5 matches tokens, not meaning; it
misses paraphrase and synonyms. Our public claim is a "complete replacement for
vector-only memory," and that is only honest if MindLeak also covers the recall
axis vectors are good at.

## Decision

Add an **optional local embedding index** as a **complementary semantic-recall
layer**. The decay graph remains the primary memory and the source of truth
(ADR-0002 is unchanged).

- **Vectors are a lens onto the graph, not the memory.** A new `recall(query)`
  tool returns semantically-similar **node ids + scores**; the agent seeds those
  into `graph_multi_hop_query` / `get_impact_radius`. Embeddings find the entry
  point; the graph does the reasoning.
- **Off the zero-token hot path (invariant 1 preserved).** Ingestion stays
  deterministic and token-free. Embeddings are produced by a separate, optional,
  asynchronous **`index`** pass — exactly like consolidation. The write path
  never blocks on, or requires, an embedding model.
- **Local and optional (like `consolidate.rs`).** Embeddings come from a local
  OpenAI-compatible `/v1/embeddings` endpoint (Ollama, LM Studio, …) and degrade
  cleanly when unreachable; nothing leaves the machine. Config:
  `MINDLEAK_EMBED_URL` / `MINDLEAK_EMBED_MODEL` / `MINDLEAK_EMBED_API_KEY`.
- **Recall-only and derived.** Embeddings store no new truth, do not affect
  decay, and gate nothing. Node content is the source; the index is a
  recomputable accelerator — dropped when a node is pruned, recomputed when its
  content changes. Eventually consistent, never a hot-path dependency.
- **Storage.** An `embeddings(node_id, model, dim, vector BLOB, updated_at)`
  table in the same SQLite database, self-managed by the embedding module
  (`CREATE TABLE IF NOT EXISTS`, so no schema/migration coupling). Recall is
  brute-force cosine over stored vectors for now; an FTS pre-filter → embed
  re-rank, or an ANN index, is a later optimization for large corpora.

### Design surface (the spec)

- `crates/mindleak-core/src/embed.rs`: an optional `Embedder` client
  (`embed(text) -> Vec<f32>`), `cosine(a, b)`, and an `EmbeddingIndex` over a
  `&Connection` (`ensure_table`, `upsert`, `recall(query_vec, limit)`).
- `MindLeak` facade: `index_nodes(limit)` (embed nodes lacking a current vector)
  and `recall(query, limit) -> Vec<ScoredNode>` (embed the query, cosine-rank,
  load nodes). Both error cleanly when no embedding model is configured.
- MCP tool `recall(query, limit)`; the maintenance pass is exposed as `index`.

## Reconciliation with ADR-0002

ADR-0002 rejected two specific things: **vectors as the primary store**, and
**per-event LLM extraction on the write path**. This ADR does neither. The write
path remains zero-token and deterministic; embeddings are an *optional, async,
off-path* complement for the *one* axis vectors win (semantic recall), proven
additive rather than competitive by the impact experiment (graph 100% vs
similarity 25%). The decay graph is still the memory; the index is a lens.

## Consequences

- **Do not** let embeddings gate ingestion, decay, prune, or impact — recall
  only. If synthesis creeps onto the write path, it is a regression of
  invariant 1.
- **Zero cost when unconfigured.** No embedding model ⇒ no index, no calls;
  identical to today. `recall` errors cleanly, like `consolidate_session`.
- The **"replacement for vector-only memory"** claim becomes honest: structure +
  time (graph) **and** semantics (index).
- **Freshness is eventually-consistent** — re-index on content change, drop on
  prune. A stale vector is a recall miss, never a correctness bug.
- **Tests must not require a network.** `cosine` and recall ranking are tested
  with injected vectors; the live `/v1/embeddings` round-trip is an `#[ignore]`d
  test, like the LLM ones.
