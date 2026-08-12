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
  the constitution was amended. Still open: sharing the lookup makes the right
  answer available, it does not make asking it compulsory — a third generator
  that simply calls `create_task` will have this bug again, and nothing in the
  type system or the tests would say so.
