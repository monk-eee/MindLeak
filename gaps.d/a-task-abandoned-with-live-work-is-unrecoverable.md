- **Abandoning a task is silent about live work, and cannot be undone —
  MEASURED, OPEN.** `task_transition to:"abandon"` is a one-way door: `reopen`
  refuses terminal work, by design, so the state is permanent. Nothing checks
  whether the task's branch has a pull request. A task can therefore read as
  dropped while its output is already on `main`, and no later action can
  reconcile the two.

  — *The measurement.* Of 109 abandoned tasks, 8 recorded a branch, and **all
  8 of those branches correspond to real pull requests** — 7 merged, 1 still
  open (`node target/tmp/abandon-audit.mjs`, 30 Jul 2026). Stated honestly:
  `branch` is the branch the *session* declared at claim time, not necessarily
  the one that carried the work, so this is evidence of a pattern rather than
  proof for each task. The pattern is nonetheless one-directional — no abandoned
  task with a branch turned out to have no pull request at all.

  — *Two instances from one day.* `task:402c153628cd` was abandoned in the
  belief its work had been superseded by a duplicate; the fix had in fact
  merged, so the ledger records dropped work that shipped.
  `task:6f17d54096c1` was abandoned while PR #262 was open with eight files of
  gap-catalogue edits. Neither was carelessness — in the first case the
  duplicate genuinely existed and the deference was correct at the moment it was
  made, and the outcome inverted afterwards.

  — *Why this is worth a check rather than a shrug.* It manufactures precisely
  the ambiguity that *"an agent can work all day and certify nothing, and the
  board cannot tell the difference between unfinished and unclosed"* already
  measures, and it does so permanently. An abandoned task cannot be completed,
  so the work it produced can never carry a receipt, and the count of abandoned
  tasks stops being readable as "work we chose not to do".

  — *The candidate repair, small.* Refuse or warn on `abandon` when the task's
  branch has an open or merged pull request, naming it — the same shape as the
  guards that already refuse a claim without a session or a push without a live
  claim. This is a check, not a redesign: the one-way door is correct and
  should stay one-way, because a reopenable terminal state is how a receipt
  becomes negotiable. What is missing is being told, at the moment of the
  irreversible act, that something live points at it.
