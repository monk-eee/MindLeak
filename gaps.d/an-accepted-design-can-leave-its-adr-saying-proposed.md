- **An accepted design can leave its ADR file saying `Proposed` — OPEN.** —
  Found 2026-07-29. `design:0059-the-tool-surface-is-a-vocabulary` was
  `accepted` by `monk-eee` and `materialized` (it spawned the tool-collapse
  tasks), while `docs/adr/0059-*.md` still read `Status: Proposed` for nine
  hours. The Design Board sync (`reconcile_designs`) reads ADR files *into* the
  ledger; nothing writes a ledger decision *back into* the file, so the two can
  disagree indefinitely. — Impact, measured the same day: an agent surveying
  claimable work read the file, judged the collapse tasks to be implementing an
  undecided design, and declined to claim any of them. The stale file did not
  merely mislead a reader; it stopped real work. — Distinct from
  `accepting-a-design-wrote-accepted-into-whichever-worktree` (that wrote the
  status into the wrong tree and is fixed); here nothing writes it at all. This
  PR flips the one file by hand; the general repair is for `accept_design` to
  update the ADR, or for a check to fail when a materialized design's ADR does
  not say `Accepted`.
