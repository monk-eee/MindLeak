- **The delivery queue no longer waits forever on a check that never started.**
  An armed pull request whose head carries no check runs at all answered
  "anything running?" and "anything failing?" exactly as a fully green one did,
  so it read as up to date and idle: the tick returned `wait` with "waiting on
  GitHub to merge it", and nothing aged it out — the stall threshold guarded
  only a branch whose checks were already running. One pull request whose
  workflow never fired could therefore hold every branch behind it
  indefinitely, and the log read like a healthy queue the whole time. An absent
  rollup is now its own state: it is still worth waiting for while it is young,
  because a run can take minutes to appear, but it ages out on the same stall
  threshold and the branch behind it takes its turn. The tick also names it —
  `#N is armed and up to date but no check has reported` — so the queue can no
  longer be silently wedged by the one thing it cannot fix itself. A branch
  that is merely behind is still updated regardless of its rollup, because that
  update is what triggers the run it is missing.
