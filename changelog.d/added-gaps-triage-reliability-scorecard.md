- **`node scripts/gaps.mjs --triage` reports the gap backlog's real reliability
  signal: age and task linkage, not just a count.** A `gaps.d/` fragment
  records that something is broken; it says nothing about whether anyone is
  fixing it, so a fragment filed on day one and one filed five minutes ago
  looked identical in `--list`. `--triage` computes each fragment's real
  filing date from `git log --diff-filter=A` (oldest surviving commit that
  added the path, immune to a later revert re-adding it) and whether its own
  text names a Lodestar `task:<id>` tracking the fix, then reports the backlog
  total, how many are orphaned (no task reference at all), and the oldest and
  median age. Measured on this repository the day this shipped: 33 open
  fragments, 24 orphaned, a 26-day median age — the honest number this repo's
  own gap-filing habit had never been asked to produce. `make gaps-triage`
  runs it.
