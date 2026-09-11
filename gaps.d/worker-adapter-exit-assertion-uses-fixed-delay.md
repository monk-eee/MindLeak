- **Worker-adapter completion test can fail after its fixed sleep - OPEN.**
  Observed 2026-09-11 in
  `crates/ackplane-supervisor/tests/worker_adapter.rs::starts_and_observes_a_running_then_exited_worker`:
  the full locked industrial run expected `Completed` at its second observation
  but received `Started`, stopping after 1,195 passing tests and one failure.
  The test starts a 1,500 ms child, sleeps for 2,500 ms, then assumes completion;
  this is elapsed-time slack, not synchronization with child startup or exit.
  The same unchanged test passed in an earlier full run and on the subsequent
  isolated run. Timing sensitivity is suspected; the exact scheduling cause is
  not established. No supervisor production code or test was changed by the
  signer workstream.

  Impact: the required industrial gate can fail before later suites execute,
  without a deterministic worker regression. Replace the fixed-delay assumption
  with bounded observation of actual process state and reliable cleanup on
  assertion failure. Left for the runtime workstream, reported on PR #922;
  rerunning successfully does not fix this gap and no test was skipped.

  PORTABLE: elapsed sleeps do not synchronize child readiness or exit. Observe
  the required event with a bounded deadline instead of assuming a scheduling
  delay proves completion.
