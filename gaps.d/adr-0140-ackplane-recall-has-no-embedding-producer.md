- **ADR-0140's Ackplane recall path has no producer: nothing outside tests ever
  writes `projected_node_embeddings`, so wiring the read half would ship a tool
  that can only ever answer nothing — MEASURED 2026-09-02 on `81461328`, OPEN.**
  Slice 2's own task text asserts that "the WRITE half of ADR-0140 already
  shipped — `migrations/0055_projected_node_embeddings.sql` creates the table and
  `crates/ackplane-server/src/projection/embeddings.rs` populates it". The second
  half of that is not true. `embeddings.rs` provides the *capability* to populate
  it; nothing invokes that capability:

  - `upsert_embedding` and `nodes_missing_embedding` have **no non-test callers**
    anywhere in the workspace (`grep` across `crates/`: every hit outside their
    own definitions is inside `#[cfg(test)]`).
  - `crates/ackplane-server` declares **no HTTP client at all** — no `reqwest`,
    no `ureq`, no `hyper` in its manifest — so it cannot reach an
    OpenAI-compatible `/v1/embeddings` endpoint even in principle. The embedder
    lives in `mindleak-core`, on the other side of the federation boundary.
  - Its three binaries are `main.rs`, `migrate.rs`, `commands.rs`; none indexes.
    `run_projection_worker` replays the ledger into `projected_nodes` and never
    embeds anything (`grep embed crates/ackplane-server/src/projection/rebuild.rs`
    is empty).

  **Why this matters more than "a feature is unfinished":** ADR-0140 decision 2
  is explicit that a projected node with no embedding row is `not_yet_embedded`,
  "a legible state, **not a silent gap indistinguishable from 'recall found
  nothing here.'**" With no producer and no surface reporting that state, an
  `ackplane-mcp` `recall` wired to this today returns an empty result for every
  query, and an empty result is exactly the answer ADR-0053 reserves for "the
  index was asked and nothing stood out". The two would be indistinguishable —
  the precise confusion decision 2 forbids — and the honest-looking empty answer
  is the more dangerous one, because it reads as a working tool with nothing to
  say rather than a pipeline with no input.

  **What is actually missing:** decision 2's second, optional pass over the
  projected set — the Ackplane-side analogue of `mindleak-core`'s
  `nodes_missing_embeddings` → embed → store loop. That needs somewhere to run
  and something to call, and the "something to call" is a cross-boundary
  question: `ackplane-server` deliberately holds no plane crate and no HTTP
  client, so whether the pass lives in the server, in a node that already has an
  embedder, or in a new component is a design decision, not an oversight to
  patch. Until it exists, `RecalledNode`-shaped read surface is premature.

  **Not fixed this run, and deliberately not worked around.** Found while
  starting `task:ddb8b1a4705b` (slice 3's proto/service/MCP plumbing); that task
  is blocked on this rather than built over it. The stage-two ranking it would
  have called is already landed and unit-tested
  (`crates/ackplane-server/src/projection/ranking.rs`), so nothing is lost by
  waiting — the decision layer is ready for a producer whenever one exists.
