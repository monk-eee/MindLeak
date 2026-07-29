- **An accepted design can leave its ADR saying `Proposed` — FOUND AND FIXED for
  ADR-0059, mechanism still open.** `design:0059-the-tool-surface-is-a-vocabulary`
  was `accepted` by `monk-eee` and `materialized` (it spawned four collapse
  tasks), while `docs/adr/0059-*.md` still read `Status: Proposed` for nine
  hours. Nothing reconciles the two directions: the sync reads files into the
  ledger, and no step writes a ledger decision back into the file. — Impact
  measured the same day: an agent surveying claimable work read the file, judged
  the four collapse tasks to be implementing an undecided design, and declined
  to claim any of them. The stale file did not merely mislead a reader; it
  stopped real work. — This entry fixes the one file. The general repair is for
  `accept_design` to update the ADR, or for a check to fail when a materialized
  design's ADR does not say `Accepted`.
