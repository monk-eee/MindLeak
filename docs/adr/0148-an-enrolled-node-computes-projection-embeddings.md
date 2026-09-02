# ADR-0148: An enrolled node computes projection embeddings; Ackplane stores and ranks them

- Status: Accepted
- Date: 2026-09-02
- Deciders: MindLeak maintainers
- Accepted: 2026-09-02 by the repository owner, authorized directly in session
  after being presented with all three options and their consequences —
  attributed human adoption, not an agent's unilateral choice.
- Related: [ADR-0140](0140-a-pgvector-recall-store-scoped-to-projected-nodes.md)
  (whose decision 2 this completes),
  [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md),
  [ADR-0084](0084-ackplane-evidence-has-explicit-trust.md),
  [ADR-0085](0085-node-enrolment-requires-proof-of-possession.md),
  [ADR-0087](0087-the-ackplane-graph-is-a-projection-not-an-authority.md),
  [ADR-0053](0053-the-graph-records-events-not-conclusions.md)

## Context

ADR-0140 built a pgvector-backed recall path over `projected_nodes`. Three
slices landed: the `projected_node_embeddings` table and its store methods, the
pgvector candidate retrieval, and the stage-two discrimination ranking that
decides whether anything is worth reporting.

None of it can run. Measured on `81461328`:

- `upsert_embedding` and `nodes_missing_embedding` have **no non-test callers**
  anywhere in the workspace.
- `crates/ackplane-server` declares **no HTTP client at all** — no `reqwest`,
  no `ureq`, no `hyper` — so it cannot reach an OpenAI-compatible
  `/v1/embeddings` endpoint even in principle.
- None of its three binaries index, and `run_projection_worker` never embeds.

ADR-0140 decision 2 describes the *shape* of the missing piece — "a second,
optional pass over that same projected set … the same two-phase shape
`mindleak-core` already uses locally (index nodes, then embed the ones missing
an embedding)" — but never says **who runs it**. That omission is what blocks
the read path, and it is not an oversight to patch: `ackplane-server`
deliberately holds no plane crate and no outbound HTTP client, so supplying an
embedder to it is a change of posture, not a wiring detail.

Recorded as `gaps.d/adr-0140-ackplane-recall-has-no-embedding-producer.md`, which
this ADR answers.

### The precedent that decides it

This looked like an open design question and is not. Ackplane already faced it
for knowledge and answered it: `RecordKnowledgeRequest` carries
`embedding_model` and `repeated float embedding`, supplied by the caller
alongside a `KnowledgeAuthentication` — an enrolled node's Ed25519 signature
with its own domain and nonce space. **Ackplane stores and ranks embeddings; it
has never computed one.** The same kind of derived ranking data, over content
the node already holds, already flows this way.

## Decision

1. **An enrolled node computes projection embeddings and publishes them.** The
   node runs ADR-0140 decision 2's two-phase loop against Ackplane: ask which of
   its projected nodes lack an embedding for a given model, embed those, publish
   the vectors. This mirrors `mindleak-core`'s local
   `nodes_missing_embeddings` → embed → store loop, which decision 2 already
   names as the shape, and reuses the knowledge path's trust model rather than
   inventing a second one.

2. **Publication is authenticated with the enrolled key, in its own domain.**
   The request carries the same shape as `KnowledgeAuthentication`
   (`signing_key_id`, `node_id`, `signed_at`, `nonce`, `signature`) with a
   distinct domain string, so an embedding publication can never be replayed as
   a knowledge record, a claim, an evidence record, or an activation. Key
   resolution, revocation, and expiry are ADR-0085's existing machinery
   unchanged; a node with a revoked key cannot publish.

3. **Ackplane computes no embedding and acquires no embedder.** It gains no
   HTTP client, no model configuration, and no inference cost. It remains a
   ledger, projection, and ranking service. This keeps ADR-0082 clause 1's
   boundary intact without argument: the federation service does not become a
   place where model inference happens on tenant content.

4. **The scope of a publication is the node's own tenant and repository.** A
   node may publish embeddings only for `projected_nodes` rows in the
   `(tenant_id, repository_id)` its completed connection challenge already binds
   it to. It never names a node the ledger replay did not produce — the existing
   foreign key on `projected_node_embeddings` enforces that, and ADR-0140
   decision 2's "never a second writer inventing nodes" is preserved: an
   embedding annotates a projected node, it does not create one.

5. **`not_yet_embedded` is the normal steady state and must stay legible.**
   Under this decision a repository whose node has not run the pass has no
   embeddings at all, which is expected rather than exceptional. Every read
   surface must therefore distinguish "nothing is embedded here" from "the index
   was asked and nothing stood out" — ADR-0140 decision 2 requires it, and
   ADR-0053 makes the second one a real answer. A recall response that renders
   both as an empty list is a defect, not a simplification.

6. **The embedded text is the projected label.** `projected_nodes` carries
   `node_type` and `label` and no content, so the label is what
   `nodes_missing_embedding` returns and the only text available to embed. A
   richer input is a later decision that would need the projection to carry more
   than ADR-0087 currently replays into it.

7. **The pass stays optional, and its absence degrades rather than fails.** A
   repository that never runs it keeps the recency/decay ranking it has today
   (ADR-0080). Recall reports its state honestly instead of erroring, and no
   Ackplane read path may require an embedding to exist.

## Consequences

- ADR-0140's read path becomes reachable: the ranking already landed
  (`projection/ranking.rs`) gains a producer, and `task:ddb8b1a4705b`'s
  proto/service/MCP plumbing has something real to return.
- Recall quality depends on nodes choosing to run the pass. That is the accepted
  cost: the alternative buys uniform coverage by putting model inference and
  outbound network egress inside the service that holds the ledger, for a gap
  that is hypothetical until nodes are observed not running it.
- A node can influence its own repository's recall ranking. This is bounded to
  content that node already authors and publishes, and is the same exposure the
  knowledge path already accepts. It is not a new trust surface.
- Different repositories may embed under different models. The `model` column is
  already part of the key, so this is representable; a query naming a model
  nothing was embedded under yields no candidates, which decision 5 requires be
  reported as such rather than as "nothing matched".
- Ackplane acquires no new dependency, deployable, or identity. The option of a
  separate indexer component remains available later if node participation
  proves insufficient, and this decision does not foreclose it.
