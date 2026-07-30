### Fixed

- **A worktree created after a server started is no longer invisible to it.**
  PR #239 made every worktree root a candidate when placing a path, which
  stopped most ingest calls being refused — but the root set was resolved once,
  at engine construction, so it was frozen for the life of the process.

  This fleet creates worktrees hourly, so a frozen set decayed from the moment
  it was resolved, and fastest exactly when the fleet was busiest. Observed
  2026-07-30: servers started at 03:55Z refusing paths from four worktrees that
  appeared later in the same session.

  A path that lands outside every known root now re-resolves the set once and
  retries the placement, so a worktree born after startup is picked up without
  restarting the server. The refresh rides on the failure that needs it rather
  than a timer, and is bounded — at most one per interval, never more than one
  in flight — so a genuinely foreign path from a misconfigured sensor cannot
  make every refusal pay for a git subprocess.

  A path under no worktree of this repository is still refused: the retry
  changes *when* the answer is computed, never what counts as belonging. The
  refresher is injected, like the roots themselves, so the core still does not
  spawn git and a test can make a new root appear without one.
