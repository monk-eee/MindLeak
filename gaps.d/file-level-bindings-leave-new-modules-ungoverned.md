- **Newly added modules arrive bound to no goal, and nothing reports it —
  MEASURED, OPEN.** `link_goal_to_artifact` binds whole files, and bindings are
  applied to the files that existed when somebody last ran the binding. So the
  ungoverned set grows with the codebase: `scripts/binding-audit.mjs` reports
  12 unbound source files today, among them
  `crates/mindleak-core/src/ingest/structure/rust.rs` and
  `crates/mindleak-core/src/graph/repair/collapse.rs`, both added recently.
  Nothing surfaces that until a person runs the audit by hand, and a file bound
  to nothing produces receipts that cover nothing. The honest options are
  symbol-level bindings or a binding step that runs against new files as they
  land; both are design decisions rather than backlog items.

  **CORRECTION.** An earlier version of this entry also blamed `drift` and
  `needs_human` verdicts on binding granularity, and said work that "plainly
  served its goal" was being reported wrongly. That was false, and it was
  recorded before it was checked. Running `advise` on the same files afterwards
  predicted the exact verdict *before any work had happened*: "this change is
  governed by `goal:durable-intent-plane...` but no covering task claims it —
  it would drift; get a covering task or review before acting", naming both
  `crates/lodestar-mcp/src/tools/mod.rs` and `knowledge.rs`. The engine was
  right every time. Those tasks carried the wrong `goal_id` because the
  ADR-0029 pre-flight, which AGENTS.md marks non-negotiable, was skipped.

  What survives from that half is one structural finding a reader can act on:
  the warning is not reachable at the moment it can be used. `advise` computes
  the drifting set, but `task_claim` cannot — it answers through
  `code_for_goal(task.goal_id)`, so it lists what your *own* goal binds and
  structurally cannot report that a declared path is bound to another goal,
  which is the only set that drifts you. A quiet claim therefore reads as
  "nothing governs this". And `also_serves` is fixed at `task_create`
  (ADR-0041) with no verb to add it later, while `paths` are only declared at
  claim — so even a perfect warning at claim time arrives after the last moment
  it could have been acted on, leaving abandon-and-recreate as the only repair.
  Note also that `drift` and `needs_human` both land `in_review`, so
  reclassifying the verdict would change nothing; only `aligned` completes.
