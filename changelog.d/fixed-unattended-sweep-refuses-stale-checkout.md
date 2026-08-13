- **The unattended artefact sweep now refuses to run from a stale checkout, as
  the manual reclaim CLI already did.** `worktree-reclaim.mjs` fetches origin
  and refuses when its own file differs from `origin/main`;
  `artefact-sweep.mjs` — which runs unattended from the delivery watcher, and
  which imports its deletion rules from that same file — had no equivalent
  check. So the deliberate command was guarded and the automatic one was not.
  Measured consequence: after the sweep was taught to spare the fleet host's
  build output, the host's `target/debug` and `target/release` were deleted
  anyway and `canonical-push` began refusing with "the Lodestar ledger is
  unreachable" — because the sweep that deleted them ran from a checkout whose
  `scripts/` predated the fix.
  `sweepIfDue` now consults `readSweepFreshness` after the due-ness check and
  before taking the fleet-wide lock, so the fetch stays off the delivery
  queue's hot path and a sweeper about to be refused never holds the lock.
  Freshness is a property of the pair: both `scripts/artefact-sweep.mjs` and
  `scripts/worktree-reclaim.mjs` must match `origin/main`, because a current
  sweep on top of stale rules deletes by stale rules and looks healthy doing
  it. An origin that cannot be fetched reads as stale, never as current.
