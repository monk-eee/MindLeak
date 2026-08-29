- **Consecutive tasks on one long-lived branch inherit each other's governed
  scope, so the second earns `drift` for the first one's files — MEASURED
  2026-08-29, OPEN.** A task's evidence bundle is built from the commits in its
  claim window on its branch. A reconciliation merge taken mid-task (bringing
  `origin/main`, or the delivery queue's own update to your branch, back in)
  produces a merge commit whose diff attributes files the *previous* task on
  that branch already committed. Those files then appear in this task's
  `changed_node_ids`, and if any of them is governed by a clause this claim did
  not declare, conformance correctly returns `drift`.

  Measured: `task:6d751963490d` changed 4 files and its evidence listed 10. The
  extra six were `task:c67ccd90a9a3`'s work, already committed and pushed on the
  same branch. One of them, `scripts/scoped-commit.mjs`, is governed by
  `goal:an-agent-commits-only-in-a-working-tree-it-owns`, a `block`-consequence
  invariant the earlier claim had declared in `also_serves` and the later one
  had no reason to. Both tasks were correct; both verdicts were correct.

  Impact: an agent running several tasks on one branch — which is the natural
  shape when the delivery queue keeps that branch alive across a session — will
  see `drift` on later tasks for work it genuinely did, under a goal it
  genuinely does serve, and the finding names a file it did not touch this time.
  That reads as a false positive and invites the wrong response.

  **The wrong response is already blocked, and that is worth knowing.**
  Re-claiming with `also_serves` after the finding is raised is refused:
  *"coverage declared after a finding is raised is a rationalisation, not a
  plan. Complete this task with the verdict it earned and carry what you learned
  into the next one."* Correct, and load-bearing — a system that lets you widen
  declared scope in response to a violation has a scope declaration that means
  nothing.

  Two real remedies, neither implemented here. The first is procedural and
  already the documented rule: one branch per task (ADR-0038), so a task's
  evidence window cannot reach work it did not do — this incident is what
  deviating from it costs. The second would be a tool change: when a claim
  starts, report the governing clauses of everything already committed on the
  branch since its base, so the coverage can be declared honestly *before* any
  work begins rather than discovered at completion. `task_claim` already
  computes governing clauses for the declared paths; extending that to the
  branch's existing diff is the same query against a wider set.

  Not fixed this run: the fix is a Lodestar-side change to what `task_claim`
  reports, and choosing between "warn at claim time" and "widen the window's
  definition" is a design decision, not a patch.
