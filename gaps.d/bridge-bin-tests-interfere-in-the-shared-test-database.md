- **`ackplane-bridge`'s binary test suite fails intermittently when run
  locally, with a different test each run, because its Postgres-backed tests
  share one database concurrently.** Observed 2026-08-28 running
  `cargo test -p ackplane-bridge --bin ackplane-bridge` against the
  Compose-provisioned `ackplane_test` database: run 1 failed
  `handlers::repository::constitution::handler_tests::replaying_a_withdrawn_proposal_is_gone_not_a_false_proposed_claim`
  plus three `..._is_404_for_an_unenrolled_repository` tests; run 2 failed one
  of them; a run with unrelated changes stashed failed
  `repository_timeline_is_404_for_an_unenrolled_repository` and
  `telemetry_route_preserves_bounded_server_buckets_recent_events_and_tenant_scope`
  instead. Every one of them passes in isolation
  (`cargo test -p ackplane-bridge --bin ackplane-bridge <name>`), and the
  varying membership is what identifies this as cross-test interference rather
  than a defect in any single test.
  Confirmed **pre-existing**: it reproduces with the current session's changes
  stashed, so it is not caused by the supervisor daemon work that surfaced it.
  Impact: `cargo test --all` is not reliably green locally, which is corrosive
  in a specific way — an agent who sees an unrelated red suite learns to
  re-run until green, and that habit is exactly how a real regression gets
  waved through. CI does not appear to hit it (these suites are green on every
  recent pull request), most likely a scheduling difference, which makes it a
  local-only trap that CI will not catch for you.
  The `..._404_for_an_unenrolled_repository` shape is the tell: those tests
  assert a repository is *absent*, so any concurrently running test that enrols
  a repository in the shared database can falsify them. ADR-0133 already
  established that a shared test database is not a test database and gave
  `ackplane_test` its own Compose-provisioned database; this is the same
  argument one level down, between tests inside that database. The likely fix
  is per-test schema or tenant isolation for the absence-asserting tests, not
  `--test-threads=1`, which would hide the coupling behind a slower suite.
  Left for later: outside the scope of ADR-0116 slice 5, and worth its own
  measurement rather than a guess bolted onto a daemon change.
