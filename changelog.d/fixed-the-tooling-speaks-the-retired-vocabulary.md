- **A guard that forbids closing a task had stopped being able to fail.**
  ADR-0065 says the completion offer *offers* and never closes, and the test
  asserting it watched for `complete_task` — a name ADR-0059 retired into
  `task_transition(to="complete")`. Proved by probe: with the module made to
  close a task through the new verb, the guard still passed. It now watches
  every verb that could close a task, including the deprecated alias, because
  closing through the alias is equally forbidden while it still answers.
  Only one of the two ADR-0065 assertions was actually vacuous — the other was
  incidentally saved by an exact-call-list comparison that noticed the extra
  call. That is luck, not coverage, and it is worth saying plainly: a guard
  named after a verb dies quietly when the verb is renamed, and the only signal
  is that it keeps passing.

- **The two messages the fleet reads most often taught retired verbs.** The
  claim gate's remediation — printed when a publish is refused, which is the
  moment an agent is most likely to copy an instruction verbatim — said
  *"Claim existing work: `claim_task(task_id)`"* and *"`create_task(goal_id,
  title, acceptance)`"*. The completion offer, printed after every successful
  push, said *"submit explicitly with `complete_task(...)`"*. All three are
  retired names. Nothing was broken, because the deprecation window still
  answers them, which is precisely why nothing noticed: advice is a string, so
  no compiler, linter or type check can see it go stale. They now name
  `task_claim`, `task_create` and `task_transition` with the argument that
  selects the act, and tests assert the verbs rather than the wording so the
  sentences stay free to improve.

- **Two more guards were written against names the surface no longer
  advertises** — the tool-surface benchmark's fixture (`next_task`) and the
  completion-offer assertions above. Recorded in Known gaps: the agent-loop
  benchmark still *drives* the retired vocabulary, so its published results
  characterise the surface agents are being migrated away from, and the
  committed scripts — including `canonical-push`, the only sanctioned publish
  path — still call retired names at 17 sites that the removal train will
  delete.
