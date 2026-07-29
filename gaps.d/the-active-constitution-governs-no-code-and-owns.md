- **The active constitution governs no code, and owns no work — MEASURED,
  OPEN.** Constitution v2 minted 25 active goals with ids suffixed
  `@constitution:v2`. Every code binding and every task still names the v1 id,
  and 25 of the 26 superseded goals record no `superseded_by`, so nothing can
  follow the rename. Measured 2026-07-29 with `node scripts/binding-audit.mjs`:

  ```
  active goals                      : 25
  active goals WITH code bindings   : 0
  bindings held by superseded goals : 156 of 156
  tasks under superseded goals      : 217 of 217
  ```

  — High impact: `governing_goals` filters to active goals, so it reports `[]`
  for files that are demonstrably bound, and `advise` answers "no active clause
  governs this change; proceed" for *every* change. That reads as approval and
  is actually the constitution being disconnected — no `forbid_change` lock can
  fire and no clause can be enforced. Conformance still works only because tasks
  and bindings are consistently on the *old* ids. — Not fixed here: re-pointing
  156 bindings and 217 tasks is a hard-to-reverse ledger rewrite on a live
  fleet, and with no recorded `superseded_by` the v1→v2 mapping would have to be
  guessed from slugs. Binding the v2 goals *without* moving the tasks would make
  every agent's evidence read as `governed code changed without a covering
  task`, i.e. drift. — **Root cause found and fixed in flight (PR #156):**
  `amend_constitution` superseded the outgoing clauses with a bare status flip
  and never set `superseded_by`, so nothing could follow the rename it performs.
  The amendment now records the successor by slug and moves bindings and
  non-terminal tasks in the same transaction, and a `run_once` migration repairs
  ledgers already in this state. It cannot be done as a sweep: bindings and
  tasks must move together or every live task reads as drift.
