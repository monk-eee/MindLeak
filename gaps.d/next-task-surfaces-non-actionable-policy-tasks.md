- **`next_task` surfaces non-actionable policy tasks.** — A `constraint` goal was
  decomposed into four tasks that merely restate the constraint and can never
  accrue completion evidence; `next_task` (oldest-first) hands one out on every
  call. — Low-medium impact: agents are handed a zombie. — **Fixed Jul 2026:**
  `decompose_goal` now returns `LodestarError::Invalid` for `constraint`/
  `invariant` goals (only `objective` goals decompose); the four restatement
  tasks were retired with `abandon_task`; regression test
  `constraint_goals_cannot_seed_junk_and_next_task_surfaces_actionable_work`.
