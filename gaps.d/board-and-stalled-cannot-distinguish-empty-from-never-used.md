- **`board` and `stalled` return the same empty `[]` for "never used" and for
  "clean, nothing outstanding".** — Observed 2026-08-17: both
  `task_query(view="board")` and `task_query(view="stalled")` return an empty
  array with no accompanying signal for either state. Where:
  `crates/lodestar-mcp/src/tools/executive.rs` (`board`, and the `"stalled"`
  dispatch arm). Impact: a caller cannot distinguish "no task has ever existed
  under this scope" from "every task here is healthy" — those are different
  facts (the first means nothing has been set up yet; the second means
  monitoring is working) and a fresh, unconfigured repository looks
  indistinguishable from a mature, fully healthy one. Left for later: needs an
  explicit count/marker (e.g. a sibling `total_tasks_ever` or a `never_used:
  true` flag) rather than relying on callers to separately query
  `lodestar_stats` to disambiguate; not fixed this run.
