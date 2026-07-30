- Build-artefact hygiene now runs itself. A fleet of worktrees rebuilds the same
  crates endlessly and nothing ever removed the result: a measured 149 GiB across
  124 cache directories, on clean branches already merged into `main`. Cleanup
  that depends on someone remembering does not happen, because the agent that
  filled a worktree has finished and moved on before it is safe to empty it. So
  the sweep has no schedule of its own — it rides on the delivery watcher
  (`make queue-watch`), which is already persistent and already single-owner:
  once at startup, then on a bounded cadence, with the last run and a lock both
  held in the common Git directory so two worktrees can never sweep at once.
  `make sweep` and `node scripts/artefact-sweep.mjs` report the same plan for
  diagnosis, and `--apply` acts. Safety is the contract: it removes only
  reproducible build output, never a worktree, source, Git state, `target/tmp`,
  telemetry, completion offers, release assets, or the bare host's
  `target/release`, which serves the running MCP binaries. It skips any worktree
  that is detached, dirty, unmerged, backing an open pull request, or active
  within the grace period, re-checks every one of those immediately before
  deleting so a plan that went stale while the disk was walked is abandoned
  rather than acted on, and counts every skip with its reason in the report.
