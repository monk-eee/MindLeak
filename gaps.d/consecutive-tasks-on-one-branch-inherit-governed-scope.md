- **Consecutive tasks on one long-lived branch inherit each other's governed
  scope, so the second earns `drift` for the first one's files — MEASURED
  twice, 2026-08-29 and 2026-08-30, OPEN.** A task's evidence bundle is built
  from the commits in its claim window on its branch — which is *every* commit
  on that branch since its base. So any earlier, still-unmerged work on the same
  branch lands in this task's `changed_node_ids`, and if one of those files is
  governed by a clause this claim did not declare, conformance correctly returns
  `drift`.

  **Correction, 2026-08-30 — the first diagnosis here was too narrow.** It
  originally blamed "a reconciliation merge taken mid-task". A merge was present
  in the first instance but is incidental: the second instance had no merge at
  all. The previous task's work sat on the branch purely because its pull
  request had not merged yet, and every subsequent task inherited it. Naming the
  merge would have sent the next reader to guard the wrong thing.

  Measured:

  | task | files it changed | files in its evidence | why |
  |---|--:|--:|---|
  | `task:6d751963490d` | 4 | 10 | prior task's commits on the branch |
  | `task:d9f855a524a2` | 5 | 20 | prior task's PR still unmerged |

  In the first, `scripts/scoped-commit.mjs` was governed by
  `goal:an-agent-commits-only-in-a-working-tree-it-owns` (a `block`-consequence
  invariant) which the earlier claim had declared in `also_serves` and the later
  one had no reason to. In the second, ADR-0144's fifteen files carried
  `goal:durable-intent-plane-for-multi-agent-coordinatio`. Every task was
  correct; every verdict was correct.

  Impact: an agent running several tasks on one branch — the natural shape when
  the delivery queue keeps that branch alive across a session — sees `drift` on
  later tasks for work it genuinely did, under a goal it genuinely serves, with
  the finding naming a file it did not touch this time. That reads as a false
  positive and invites the wrong response. The real damage is not the noise: it
  is that the same verdict is load-bearing when it is true, and an agent taught
  to discount it will discount it then too.

  **The wrong response is already blocked, and that is worth knowing.**
  Re-claiming with `also_serves` after the finding is raised is refused:
  *"coverage declared after a finding is raised is a rationalisation, not a
  plan. Complete this task with the verdict it earned and carry what you learned
  into the next one."* Correct, and load-bearing — a system that lets you widen
  declared scope in response to a violation has a scope declaration that means
  nothing.

  Two real remedies, neither implemented here. The first is procedural and
  already the documented rule: **one branch per task** (ADR-0038), so a task's
  evidence window cannot reach work it did not do — these two incidents are what
  deviating from it costs. The second would be a tool change: when a claim
  starts, report the governing clauses of everything already committed on the
  branch since its base, so coverage can be declared honestly *before* any work
  begins rather than discovered at completion. `task_claim` already computes
  governing clauses for the declared paths; extending that to the branch's
  existing diff is the same query against a wider set.

  Not fixed this run: the fix is a Lodestar-side change to what `task_claim`
  reports, and choosing between "warn at claim time" and "narrow the window's
  definition" is a design decision, not a patch.
