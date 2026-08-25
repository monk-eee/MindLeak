- **`board-health.mjs` now surfaces orphaned `gaps.d/` fragments alongside the
  claimed-task report.** `classify`/`describe` already separated the board's
  own blind spots (unresolvable vs. decidable `needs_human`, lapsed claims,
  shipped-but-open tasks); the same blind spot existed for gaps.d, a fragment
  recording that something is broken says nothing about whether anyone is
  fixing it, and nothing surfaced that automatically. Reuses `gaps.mjs`'s own
  `triageReport`/`firstAddedDates` (no duplicated logic) to report, every time
  `board-health` runs: how many open fragments have no `task:` reference at
  all, the oldest and median age, and up to 10 named. Silent when `gaps.d` is
  empty -- an empty directory is not a finding.
