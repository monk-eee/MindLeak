- **`make board-health` separates work a human must decide from work nobody
  can.** ADR-0058 decision 4 says the board should report what it cannot close;
  this is that report. `needs_human` was one verdict covering two unrelated
  situations — conformance found something arguable, or the evidence bundle was
  empty and there is nothing to rule on at all. It also names stranded claims:
  a lapsed lease still holding scope against other agents (ADR-0048).
  Measured on this repository, 207 tasks: **0 decidable, 0 unresolvable, 27
  stranded**. The first draft of this report said 51 parked instead of 0,
  because a task keeps its conformance audits after it finishes and classifying
  by "latest audit" alone counted completed work as pending — every one of
  those 51 was already `done` or `abandoned`. Inflating a backlog is not a
  milder failure than hiding one; it sends people looking for work that does
  not exist. Terminal tasks are now excluded, with a test.
  Reporting only: nothing here closes, abandons, or reassigns anything, because
  ADR-0058 decision 5 is explicit that nothing closes automatically.
