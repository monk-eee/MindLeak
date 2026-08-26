- **The board hands one ADR to several agents at once.** — `decompose_goal`
  created a task per draft on every run, so repeated runs left three or four
  identical `Implement: ADR-NNNN` seed tasks per ADR (34 live tasks across eight
  ADRs on 2026-08-11) and `task_query view=next` serves them independently, so
  two sessions each legitimately own a different seed for the same decision.
  Observed live: `task:99a586a4075c` and `task:7fe8a3f0b7ae` were both ADR-0090;
  the first claim's `view=overlap` preflight returned zero claims because the
  second claim did not exist yet, and by the time that owner reached its first
  edit the peer had already written
  `crates/lodestar-core/src/facade/conformance/certification.rs` on
  `feat/certification-status`. — High impact on coordination: the overlap
  preflight is the mechanism this repository offers against duplicate work and it
  cannot cover this case, because a claim-time check is a race the second
  claimant always wins. Duplicated effort is the visible cost; the invisible one
  is that both branches edit the same governed files and only one of them can
  merge. — Fixed at both layers: `decompose_goal` is idempotent per
  `(goal, title)` over live work, and the store no longer decides duplication by
  the clock — it used to be answered by the derived id colliding, and that id
  hashes the creation second, so an identical title was refused inside one second
  and allowed a second later. Design materialization in create mode now resolves
  a draft to live work of the same goal and title too, which closes the repair
  path: a revision re-stating its drafts used to build twins of the tasks the
  previous revision created and leave the originals live but unreachable, because
  `design_task_links` are deleted before the drafts are created. `promote_design`
  itself was never the hole the earlier text claimed — plan equality and the CAS
  on `promotion_status` already make a straight replay idempotent. The seeds
  already on the board when this was found were retired separately. Both
  generators now share one slug-matched lookup rather than the two divergent
  implementations they started with, which would have disagreed the first time
  the constitution was amended. `task_create` reports an exact-title live match
  in its own field, because naming the prior work was not enough on its own: the
  list it was buried in is every task ever created under the goal, 203 of them
  when this was measured, and two agents created "Make worktree reclaim refuse
  loudly when the Lodestar board is unreadable" against one goal with that report
  already in front of them. Then the board refilled anyway: 28 seeds in one pass
  on 2026-08-12, because a generator was run once per active goal and each ADR
  produced one identically titled task under every objective, in the same second.
  Four copies of "Implement: ADR-0086: PostgreSQL is the Ackplane ledger", of
  which three named goals a PostgreSQL arbiter does not serve. A per-goal
  comparison cannot see that by construction, so `task_create` now reports a
  same-title live task under another goal separately, naming the goal it already
  serves. — Still open, and unchanged by any of it: every report here is advisory
  under ADR-0015, so an agent that ignores one still creates the duplicate, and
  nothing reconciles the result afterwards — all 28 were retired by hand.
  **The "deeper cause" sentence that followed here is now stale — verified
  24 Aug: ADR-0092 was Accepted and adopted 2026-08-13 as
  `amendment:a334b7f2c123`, creating `goal:ackplane-federation-service@constitution:v4`
  and binding it to Ackplane's crates.** Work generated under "whichever goal
  was to hand" for Ackplane specifically is closed by that adoption, not merely
  proposed — routine `decompose_goal`/`task_create` calls against that goal id
  in this fleet confirm it is live and in ordinary use. What ADR-0092 did not
  touch, and what is still genuinely open, is the sentence before it: every
  duplicate-task report in this fragment is advisory under ADR-0015, so an
  agent that ignores one still creates the duplicate, and nothing reconciles
  the result afterwards. That is a distinct question (should a same-title,
  same-goal `task_create` be refused rather than merely flagged?) from the one
  ADR-0092 answered (which goal does a subsystem's work belong to?), and it
  remains unaddressed.
  **Fresh instance, 2026-08-26: reconciliation still does not happen on its
  own, and this time nobody automated it away.** Two sessions independently
  built complete, incompatible implementations of ADR-0119 (Industrial
  administration) at the same time on different branches
  (`agents/reclaim-task-functionality`, which shipped Snapshot/Purge/Recovery/
  Export against real providers as PR #769; `agents/check-mindleak-installation`,
  which built a parallel `administration_store`/`administration_policy_store`
  plus two newly-authored ADRs, 0129 and 0131, layering more decisions onto a
  store that would never merge). `check_overlap` correctly reported the other
  session's live footprint on the overlapping Bridge files throughout, and the
  losing branch deliberately worked in separate files specifically to avoid a
  collision -- the overlap tool did its job. What it cannot do is tell either
  session that the *other one would win the merge*, because that is decided
  later, by whichever branch's PR clears CI and the merge queue first, not by
  who claimed or who wrote more. The losing branch's five Lodestar tasks were
  each completed correctly, with real evidence, and are still (correctly)
  `status: done` -- the work happened and was proven at the time. Nothing then
  told that session its branch had lost the race; that was only discovered
  because a later, unrelated question ("what's next") happened to prompt a
  check of the board and PR list. Concretely fixed this round: the losing
  branch's real remaining value -- a genuine self-approval defect the winning
  PR introduced (Lifecycle purge's confirm route accepted the same
  requesting principal as its own confirmer, contradicting ADR-0119 decision
  7 -- turned into a targeted fix (PR #779) landed directly against the
  winning branch's real code, and the losing branch's now-dead commits and
  ADRs were recorded as durable knowledge and abandoned rather than merged,
  rebased, or resurrected. Still open: nothing surfaces "a branch you have
  commits on just lost a merge race" on its own; an agent (or its user) has to
  think to go check.
