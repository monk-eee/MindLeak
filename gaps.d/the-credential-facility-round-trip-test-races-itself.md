- **`ackplane-client`'s Windows Credential Manager round-trip test is flaky —
  OBSERVED 2026-08-19, left OPEN.**
  `auth::tests::a_stored_seed_round_trips_through_the_real_credential_facility`
  (`crates/ackplane-client/src/auth.rs`) stores a seed via the real OS
  credential facility, then immediately reads it back and asserts it matches.
  Running the crate's tests with default (parallel) threading fails it
  roughly 1 time in 4 with `Facility(NoEntry)` — the entry it just stored
  cannot be found. Confirmed unrelated to any in-flight change: reproduced on
  a clean `origin/main` checkout (via `git stash`) across 4 consecutive runs,
  1 failure. `--test-threads=1` never reproduces it.

  Likely cause (not yet confirmed): another test in the same `auth::tests`
  module exercising the same credential-facility service/account name (e.g.
  `a_missing_credential_is_reported_as_a_facility_error_not_a_panic`, which
  deletes an entry) running concurrently in a different thread, racing this
  test's store-then-load. `keyring` 2.x's Windows Credential Manager backend
  may not serialize concurrent access to the same entry the way this test
  assumes.

  Impact: a routine `cargo test -p ackplane-client --all-features` (or any
  workspace-wide run including this crate) can fail on an unrelated change
  roughly 1 run in 4, wasting time re-verifying that the failure is not the
  change under test. Left OPEN: no fix attempted this run (out of scope for
  the task that found it). The right-sized fix is most likely giving each
  test in this module its own unique credential service/account name (the
  same isolation principle this crate's own Postgres-gated tests already use
  via `uuid_ish()`), or serializing the module's tests with a shared mutex if
  a unique-identity fix is not viable.
