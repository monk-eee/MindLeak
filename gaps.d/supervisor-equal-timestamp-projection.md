- **A completed worker can remain Started in the server's current session
  view when both events occur in the same second.** During the 2026-09-14
  stabilization run on `e48a3b44`, the real
  `browser_scoped_work_runs_real_supervisor_and_review_does_not_complete_task`
  workflow recorded its child completion and retained the Completed lifecycle
  receipt, but a diagnostic assertion on the final `list_sessions` view
  returned Started. `SupervisorStore::record_lifecycle` advances that view only
  for `occurred_at > current_occurred_at`, while the protocol timestamps have
  second precision. This also explains why using that mutable view to reopen an
  original Started outbox only fails on some timings. The fixture is being
  corrected separately; lifecycle receipt ordering and equal-time projection
  semantics remain open and require a focused production decision/regression.
  Do not discard accepted receipts, add sleeps to make seconds differ, or
  silently redefine ordering as part of a test-fixture repair.
