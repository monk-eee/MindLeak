- **The recall ranking change is measured against a real index, including the
  part of it that did not work.** ADR-0075 shipped on deterministic unit tests
  whose fields were synthetic and uniform. A real index is neither, so it was
  measured against this repository's own — 19,317 embedded nodes, ten queries,
  the pre-change algorithm as the control arm and the built binary as the
  treatment arm.

  Two claims held. Hits naming a node the graph no longer holds fell from **24
  of 50 to 0 of 49**: nearly half of what recall used to hand back was an id the
  caller could not open. Recorded conclusions rose from **14% of hits served to
  96%**, where they had been outnumbered five to one by symbols, executions and
  dangling references.

  One did not, and it is recorded with equal weight because the fixtures could
  not see it: **a nonsense query is still answered rather than met with
  silence.** Top-hit distance above the field is 3.11–3.90 standard deviations
  for nonsense controls and 3.71–6.21 for real questions, so the bands overlap
  by 0.19σ and no single threshold rejects one while keeping the other. The
  shipped 1σ cut sits far below both. The reasoning that failed was that
  nonsense lifts a field uniformly — true of the fixture, false of a diverse
  19,000-node index, where even nonsense has relative outliers.

  The constant is deliberately **not** tuned in response: three samples
  separated by a negative margin is the same global constant the floor
  measurement already warned against, one level up. ADR-0075 is still Proposed
  and carries a correction saying so.

  New: `scripts/evaluate-recall.mjs`, with unit tests, reproducing all of the
  above. It needs a populated index and a reachable embeddings server — both
  optional parts of the product (ADR-0008) — and reports rather than fails when
  either is absent.
