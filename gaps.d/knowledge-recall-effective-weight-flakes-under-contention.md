- **`knowledge_store::tests::a_recorded_statement_recalls_by_recency_with_no_embedding`
  flakes under full-suite contention, asserting `effective_weight <= 1.0`.**
  Confirmed via 5 consecutive isolated runs (all passed) versus a failure
  observed only inside the full `cargo test -p ackplane-server --lib`
  158-test parallel run (a pre-push isolated-hook run, not this task's own
  diff -- `knowledge_store.rs` was not touched here). The shape matches this
  repository's own documented contention-flake class (the maintenance-runtime
  SQLite flake, the ackplane-client credential-facility flake): a decay
  calculation reading two timestamps (a recorded `confirmed_at` and `now`)
  can see near-zero or slightly negative elapsed time under CPU/scheduling
  contention, and `effective_weight = base * 2^(-elapsed/half_life)` can
  therefore compute fractionally above `1.0` when elapsed drifts negative
  by even a few milliseconds of clock/scheduling skew.

  Impact: the isolated pre-push hook occasionally blocks an otherwise-clean
  push with an unrelated failure, and a retry (which re-runs the whole
  suite fresh) is the workaround used here.

  Not fixed this run: out of scope for a Fleet-pagination task, and the real
  fix (most likely clamping the computed weight to `1.0`, or asserting
  `<=` a small epsilon above `1.0` rather than exactly `1.0`, mirroring how
  `effective_weight` is defined in `mindleak_core::decay`) deserves its own
  narrow task rather than a drive-by edit to an unrelated file.
