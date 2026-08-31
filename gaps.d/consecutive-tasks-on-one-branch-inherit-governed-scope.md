- **Consecutive tasks on one long-lived branch inherit each other's governed
  scope, so the second earns `drift` for the first one's files — MEASURED
  twice, 2026-08-29 and 2026-08-30; the claim-time warning shipped 2026-08-31
  (ADR-0147), PARTIALLY OPEN for the editor-MCP path.** A task's evidence
  bundle is built
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

  Two real remedies. The first is procedural and already the documented rule:
  **one branch per task** (ADR-0038), so a task's evidence window cannot reach
  work it did not do — these two incidents are what deviating from it costs.
  The second was a tool change: when a claim starts, report the governing
  clauses of everything already committed on the branch since its base, so
  coverage can be declared honestly *before* any work begins rather than
  discovered at completion.

  **The tool change shipped, 2026-08-31.** [ADR-0147](../docs/adr/0147-a-claim-reports-the-branchs-existing-governed-files.md)
  chose "warn at claim time" over "narrow the window's definition", and
  deliberately left evidence semantics untouched (decision 5: narrowing
  `changed_node_ids` would require trusting an agent's own memory of which
  commit belonged to which task, which is the property ADR-0009 exists not to
  depend on). What landed:

  - `task_claim(step="claim")` accepts an optional `branch_committed_paths`,
    and reports the clauses governing those paths in a separate
    `branch_inherited` field — never merged into `governing`, so "this governs
    what I am about to change" stays distinguishable from "this already governs
    something sitting on my branch" (PR #867).
  - `scripts/mcp-direct.mjs` computes `git diff --name-only <base>...HEAD` and
    supplies it, so the direct-drive path declares it without being asked
    (PR #869 follow-up). Before that, nothing supplied the argument and the
    field was unreachable in practice.

  It is advisory and never gates a claim, and an absent declaration degrades to
  exactly the previous behaviour rather than to an empty one — an unknown diff
  reports nothing rather than asserting the branch carries nothing.

  **Still open: the editor-MCP path has no script in the loop.** A session
  driving Lodestar through its editor's own MCP connection — the normal case,
  not the `mcp-direct.mjs` recovery path — has no place to hook the `git diff`,
  so the agent must pass `branch_committed_paths` itself when claiming a task
  on a branch that already carries work. Until that is either wired or made
  unnecessary, the procedural rule above (one branch per task) remains the
  actual defence, and this fragment stays open for that half.
