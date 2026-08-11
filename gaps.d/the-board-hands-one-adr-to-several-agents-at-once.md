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
  merge. — Generator fixed this run: `decompose_goal` is now idempotent per
  `(goal, title)` over live work. The seeds already on the board when this was
  found are retired separately, and nothing yet prevents the same shape arriving
  from `promote_design`, which creates its tasks by another path.
