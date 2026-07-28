- **`make board-health` separates work a human must decide from work nobody
  can.** ADR-0058 decision 4 says the board should report what it cannot close;
  this is that report. `needs_human` was one verdict covering two unrelated
  situations — conformance found something arguable, or the evidence bundle was
  empty and there is nothing to rule on at all. The first measured run:
  **11 decidable, 40 unresolvable, 28 stranded claims** out of 195 tasks, so
  78% of parked work was lost work wearing the label of a pending decision.
  Sharper still, all 11 decidable items carried the *same* finding — "evidence
  does not touch code bound to the task goal", which is ADR-0060's subject — so
  the board's fifty-one pending decisions contained no judgement calls at all.
  Reporting only: nothing here closes, abandons, or reassigns anything, because
  ADR-0058 decision 5 is explicit that nothing closes automatically.
