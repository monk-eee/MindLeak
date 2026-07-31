- **`recall` depends on someone choosing to record a conclusion, and cannot
  return one until the index has caught up — NARROWED 2026-07-31, OPEN.** —
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

  **(b) Fixed 2026-07-31 — semantic hits must ground the query.** The floor and per-query sigma cut could rank but could not decide whether an answer existed: the real and nonsense bands overlapped. `recall` now requires its returned node text to support a majority of the query's IDF-weighted informative terms, while queries with fewer than three such terms keep fuzzy behavior. On the expanded live index, all three gibberish controls and four coherent absent-domain questions returned `[]`; all five query sets whose returned labels were genuinely relevant remained answered. Two old "real" cases now abstain because inspection showed they had returned generic report scripts and merge commits, not answers. The gate adds no LLM or embedding call and leaves both similarity constants unchanged. Measurement: `benchmarks/results/2026-07-31-recall-grounding.json`.

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
