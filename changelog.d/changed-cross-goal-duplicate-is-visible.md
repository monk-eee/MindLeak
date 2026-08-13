- **`task_create` now names the same title living under another goal.** The
  same-goal duplicate report could not see the shape that actually refilled this
  board: a generator run once per active goal produced one identically titled task
  under each, in the same second — 28 seeds in a single pass, including four copies
  of "Implement: ADR-0086: PostgreSQL is the Ackplane ledger" of which three named
  goals a PostgreSQL arbiter does not serve. A per-goal comparison cannot detect
  that by construction, so it is reported separately in
  `same_title_under_other_goals`, naming each task and the goal it already serves.
  It reports rather than refuses: one piece of work serving several goals is
  legitimate, but it is declared with `also_serves` on a single task (ADR-0041),
  not by forking one task per goal — only one of them can be the work, and the rest
  are graded against goals they do not serve.
