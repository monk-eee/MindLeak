- **`recall` depends on someone choosing to record a conclusion, and cannot
  return one until the index has caught up — NARROWED 2026-07-30, OPEN.** —
  Measured 2026-07-27 against this repository's graph: 4,463
  nodes, 9,572 active edges. Four queries, each naming a lesson that session had
  genuinely cost hours to learn, returned only `execution:` command lines and
  `symbol:` function names. Three causes were confirmed in the code. Two have
  since been repaired and one survives; the fragment is narrowed onto the
  residual rather than deleted, and each claim below was re-checked against
  `origin/main` rather than assumed.

  **(a) Narrowed — conclusions are recorded now, but only ever deliberately.**
  The original claim was that none is ever recorded, so nothing exists to match.
  That is no longer true: the live graph holds **478 `intent` nodes**, and
  ADR-0053 shipped `record_architectural_decision` together with the floor that
  lets recall answer nothing. What survives is structural and by design: the
  zero-token write path (invariant 1) can only capture what a machine emitted,
  so it cannot distil a conclusion by itself. Every sentence in the graph is
  there because an agent chose to put it there, and nothing in the loop compels
  that choice — so the failure mode is no longer "confident noise" but a
  silence that is only as complete as the last agent's diligence.

  **(b) Fixed.** `recall` scored every embedded node, sorted, truncated to
  `limit`, and so always answered however unrelated the field was; the nonsense
  query `zzzzqqq wibble flarp` scored **0.54**, higher than any of the four real
  questions. ADR-0053 added the floor, and ADR-0075 added a per-query
  distinctiveness cut built on exactly that measurement — an absolute constant
  cannot judge a score whose baseline moves with the query, so a candidate must
  now stand out from its own query's field. A question with no answer in the
  index is answered with silence.

  **(c) Narrowed from absence to latency.** A node was invisible to `recall`
  until the offline `index_nodes` pass embedded it —
  `record_architectural_decision` wrote `intent:8ac3a2338d52` and `recall` for
  its own title returned `[]`. The index now self-populates: the maintenance
  runtime (`crates/mindleak-mcp/src/maintenance/runtime.rs`) calls
  `index_nodes` on idle. So a just-written conclusion is recallable after the
  next background pass rather than never — but not immediately, and an agent who
  records a decision and asks for it in the same breath still gets `[]`. The
  window is bounded and no longer requires a manual call; it is not zero.

  Impact, restated for what is left: the memory half of the product no longer
  returns confident noise, and it beats a flat markdown file at ranking what it
  holds. It is still only as good as what agents choose to write into it.
  `record_knowledge` and `record_architectural_decision` were called **zero
  times in an eight-hour session** when this was first measured, because nothing
  in the loop asks — that remains the live risk, and it is a prompting and
  workflow problem rather than a retrieval one.
