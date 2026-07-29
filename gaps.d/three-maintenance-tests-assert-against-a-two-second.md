- **Three maintenance tests assert against a two-second wall clock and will
  flake on a loaded machine — OPEN.** All three live in
  [`maintenance/runtime.rs`](crates/mindleak-mcp/src/maintenance/runtime.rs) and
  share one shape: do work, then poll until an expected telemetry event appears,
  bounded by `Instant::now() + Duration::from_secs(2)`.

  | test | what it waits for | configured cadence |
  |---|---|--:|
  | `enabled_worker_runs_after_idle_and_joins_cleanly` | `total_events > 0` | idle 10 ms |
  | `active_request_blocks_idle_pass_until_completion` | the idle pass to be held off, then run | idle 10 ms |
  | `prune_runs_on_its_cadence_despite_continuous_request_activity` | an `autonomous_prune` event | prune interval 10 ms |

  Two seconds against a 10 ms cadence is generous in isolation and meaningless
  under contention: this repository is routinely worked by a fleet of worktrees
  running concurrent `cargo` builds that hold the package-cache lock, and a
  shared CI runner is no calmer. All three fail *red* rather than passing
  silently — the third uses the deadline as a loop bound but still asserts
  `pruned` afterwards, so a timeout is reported rather than swallowed. That is
  the good case; a wall-clock bound that let a test pass without observing
  anything would be far worse. — Impact: a spurious red on a pull request that
  changed nothing related, which is the kind of failure that teaches people to
  re-run CI instead of reading it, and that habit is what makes a real failure
  cheap to ignore. Three tests means roughly three times the exposure per run,
  and they are in the same file, so a loaded machine tends to trip more than one
  and make the failure look like a real regression in maintenance. —
  Not fixed this run: the honest repair is to wait on a signal from the worker
  rather than on elapsed time, and that means giving `MaintenanceRuntime` a test
  seam it does not currently have. Raising the timeout would only lengthen the
  odds, not remove them.
