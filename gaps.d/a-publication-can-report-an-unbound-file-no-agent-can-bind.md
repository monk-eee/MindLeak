- **Publication reports a newly added file as unbound, and no agent-reachable
  verb can bind it — OBSERVED 2026-08-01, left OPEN.** With the binding audit
  now running cleanly (see
  [`binding-coverage-is-dark-before-migration.md`](binding-coverage-is-dark-before-migration.md),
  which records the *previous* failure — that the control was silently dark), it
  correctly reported `UNBOUND crates/lodestar-core/src/embed.rs` at publication,
  and the completion receipt for `task:87b6e88e1da7` carried the finding
  `evidence does not touch code bound to the task goal`. Both are right. Neither
  can be acted on from where the agent is standing.

  `scripts/binding-audit.mjs` reads bindings from `goal_artifacts`, but no tool
  under `crates/lodestar-mcp/src/tools/` writes that table — grepping the tool
  definitions for `goal_artifacts` or any `bind` verb returns nothing. Bindings
  arrive by some other route (constitution adoption / design materialization),
  so an agent that has just been told its new module is ungoverned, at the exact
  moment it holds all the context needed to say which goal governs it, has no
  way to record that. It is not a matter of willingness.

  Impact is the shape this repository keeps rediscovering: an advisory nobody
  can act on trains its readers to scroll past the publication output, which is
  the same output that carries findings they *can* act on. The backlog is not
  small either — 22 files in `crates/lodestar-core/src` alone are unbound,
  including `model/knowledge.rs` and all six `store/coordination/` modules, so
  conformance's "which goal governs this changed file" question already answers
  "none" for a substantial part of the Intent Plane. Worth noting too that the
  receipt read `aligned` *while* carrying the finding, so the verdict does not
  make the gap visible either.

  Distinct from the two neighbouring entries and deliberately filed separately:
  `binding-coverage-is-dark-before-migration.md` is about the audit failing to
  run, and `the-engine-was-ungoverned-and-the-gate-that-would-enforce-it.md` is
  about coverage breadth and manifest export. This one is about the write path —
  the audit runs, the answer is correct, and there is no verb to fix it with.

  Fix direction (left for later, needs a decision on who may bind): expose a
  narrow `bind_code` verb that writes `goal_artifacts` for the ids an agent
  declares, or have `task_transition` bind the evidence paths to the task's goal
  on an aligned completion — the moment the goal and the files are both known
  and already attributed. Recording the observation this run, not fixing it.
