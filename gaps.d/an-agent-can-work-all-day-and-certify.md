- **An agent can work all day and certify nothing, and the board cannot tell the
  difference between unfinished and unclosed — MEASURED, OPEN.**
  — *The measurement.* 48 of 101 `done` tasks rest on a `needs_human` receipt
  rather than an affirmed one (`knowledge:d9ad8b8911d7`). Thirty-three claims sit
  lapsed on the board. An audit against `origin/main` on 29 Jul 2026 found at
  least **nine** of those tasks already fully implemented in main — all five
  module-split tasks, plus PRs #100, #110, #114 and #116 — while still showing as
  open or free to re-claim (`knowledge:93679dfca687`). This session added a
  tenth: `task:219184500419` shipped as PR #149, merged, and still completed
  `needs_human`.
  — *The impact.* The board is not a statement of what is missing. An agent that
  trusts it re-implements shipped work, and "done" does not mean "affirmed", so
  the completion count cannot be read as delivery. Both failure modes are
  silent: nothing warns you that the task you just claimed is already in main.
  — *Why the guard is correct.* The temptation is to blame `check_conformance`
  for refusing, and to loosen it. Do not. It requires evidence to fall inside
  the claim window and refuses to upgrade a verdict it cannot substantiate;
  without that, a receipt could be back-dated, or could cover another agent's
  commits, and would certify nothing at all. The guard is the only reason a
  receipt means anything. Every failure above is *upstream* of it — an orphaned
  goal, a stale server binary, commit-then-claim ordering — and each is fixable
  without touching the guard.
  — *The candidate repair.* Three, in order of value. (1) Re-bind the 51 goals
  orphaned when constitution v2 dropped every goal-to-code link, so
  `touched_task_goal` is answerable at all; ADR-0060 item 3 now lets a goal bind
  the docs, ADRs and benchmarks it delivers, so this is finally expressible.
  (2) ADR-0064 (the log is the ledger), so "evidence inside a *prior* claim by
  the same agent" becomes answerable and shipped work stops needing a human.
  (3) `existing_work` (`task:b8ca6e0ca5fb`), so a claimant is told the
  capability is already in main before doing the work twice. Explicitly **not**
  a repair: raising the 300-second default lease — ADR-0052 considered and
  rejected that, and a longer lease only widens the window it fails to police.
