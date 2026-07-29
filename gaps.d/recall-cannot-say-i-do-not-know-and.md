- **`recall` cannot say "I do not know", and never returns a conclusion because
  none is ever recorded — OPEN, recorded as [ADR-0053](docs/adr/0053-the-graph-records-events-not-conclusions.md)
  (Proposed).** — Measured 2026-07-27 against this repository's graph: 4,463
  nodes, 9,572 active edges. Four queries, each naming a lesson that session had
  genuinely cost hours to learn, returned only `execution:` command lines and
  `symbol:` function names. Three causes, all confirmed in the code:
  **(a)** the zero-token write path (invariant 1) can only capture what a machine
  emitted, so no sentence exists in the graph to match;
  **(b)** `recall` is cosine similarity over the ADR-0008 embedding index with
  **no floor** — `embed::recall` scores every embedded node, sorts, truncates to
  `limit`, and so always returns `limit` rows however unrelated. The nonsense
  query `zzzzqqq wibble flarp` scores **0.54**, higher than any of the four real
  questions scored;
  **(c)** a node is invisible to `recall` until the offline `index_nodes` pass
  embeds it — `record_architectural_decision` wrote `intent:8ac3a2338d52` and
  `recall` for its own title returned `[]`.
  Impact: every consumer gets confident noise instead of an empty result, and
  the memory half of the product loses to a flat markdown file for the one job
  it exists to do. `record_knowledge` and `record_architectural_decision` were
  called **zero times in an eight-hour session**, because nothing in the loop
  asks. Not fixed this run — ADR-0053 proposes the fix and is deliberately
  Proposed, not accepted.
