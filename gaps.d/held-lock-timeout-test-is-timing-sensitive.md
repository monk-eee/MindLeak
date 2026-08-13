- **The held-lock WAL retry test can fail on elapsed time alone under a busy
  Windows runner.** `crates/mindleak-core/src/db.rs` test
  `held_lock_does_not_multiply_wal_retry_timeout` took 12.013 seconds and failed
  its bounded-window assertion during `cargo test --all`, then passed unchanged
  when rerun alone through Unit Test MCP. This can make an unrelated full-suite
  validation fail nondeterministically; left for a focused timing-test repair
  rather than changing MindLeak database behavior in the goal-bootstrap work.
