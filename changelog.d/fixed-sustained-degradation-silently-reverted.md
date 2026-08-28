- **Fixed:** `open_session`'s sustained-degradation notice (`degraded`), shipped
  in PR #801, had been silently and entirely reverted by PR #806's merge: the
  `mod degradation;` declaration and its `pub use` in
  `crates/mindleak-core/src/telemetry/mod.rs`, the `MindLeak::sustained_degradation()`
  facade method, the `open_session` dispatch block that surfaced
  `body["degraded"]`, and the feature's own regression test were all dropped
  in the same commit, leaving `crates/mindleak-core/src/telemetry/degradation.rs`
  as dead code that compiled into nothing and whose tests never ran. No build
  failed and no test went red, because an orphaned, undeclared module is
  invisible to `cargo test` rather than a compile error. All four pieces are
  restored verbatim from the PR #801 merge. While restoring it, also fixed a
  latent bug in its own SQL: the reported `detail` for a sustained outage was
  `MAX(CASE WHEN outcome = 'skipped' THEN detail END)` across every skip this
  tool ever recorded — SQLite's `MAX` on a TEXT column is a lexicographic
  (byte-wise) comparison, not a recency one, so with two differently-worded
  skips the *older* one's message could shadow the tool's actual latest reason.
  Fixed by reading the detail off the row already pinned to the latest event id.
