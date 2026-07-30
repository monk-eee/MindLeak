- **Goal-to-code bindings are file-level and fixed to the tree as it was, so
  correct work reports as ungoverned — MEASURED, OPEN.** `link_goal_to_artifact`
  binds a whole file, and bindings are applied to the files that existed when
  somebody ran the binding. Two consequences, both of which make a conformance
  verdict say something other than what it means.

  A file bound to one goal reports *any* change to it as touching that goal.
  `crates/lodestar-mcp/src/tools/mod.rs` carries a binding to
  `goal:durable-intent-plane`, so a task serving a different goal that edits
  the session tables in that file lands `drift` — "governed code changed
  without a covering task" — even though the change is exactly what the task
  asked for. There is no verb that adds coverage after a claim; `also_serves`
  must be declared at `task_create` (ADR-0041), so by the time the verdict
  explains the problem it is already too late to fix it on that task.

  And a file bound to nothing reports as covering nothing. Measured across four
  task closures on 2026-07-30, two came back `needs_human` with "evidence does
  not touch code bound to the task goal" for work that plainly served its goal.
  An earlier measurement in this file recorded the scale: 8 governed nodes at
  03:37Z rising to 161 by 09:29Z, with 72 of 172 receipts still covering
  nothing. `scripts/binding-audit.mjs` re-measures it.

  The structural half is the part that will not fix itself: because bindings
  are applied to the tree as it was, **every newly added module arrives
  ungoverned**, and nothing reports that until a person runs the audit by hand.
  So the ungoverned set grows with the codebase while the receipts quietly get
  weaker. The honest options are symbol-level bindings, or a binding step that
  runs against new files as they land. Both are design decisions rather than
  backlog items, which is why this is recorded rather than patched: raising
  each verdict individually would only teach agents that `needs_human` is
  noise to be worked around, and that is the one reading that must not become
  true.
