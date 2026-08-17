- **`ackplane-server`'s Postgres-gated tests are not isolated from a
  persistent database, so a second run against the same container fails on
  leftover state. MEASURED 2026-08-17, left OPEN — out of scope for the
  `bounded_neighborhood` LIMIT-parameter task that surfaced it.**

  `enrollment_store::tests::activation_reuses_its_live_challenge_and_exact_replay_receipt`
  calls `store.issue_challenge(&request, &[1; 32], now)` — a fixed, hardcoded
  nonce, deterministic by design for the test's own assertions. Against a fresh
  database this passes. Run a second time against the *same* running
  `docker-compose` Postgres container (the normal way to validate a fix
  locally, per `ACKPLANE_TEST_DATABASE_URL`), it fails:

  ```text
  thread 'enrollment_store::tests::activation_reuses_its_live_challenge_and_exact_replay_receipt'
  panicked at crates\ackplane-server\src\enrollment_store.rs:1063:14:
  approved request receives challenge: Database(Error { kind: Db, cause: Some(DbError {
    code: SqlState(E23505), message: "duplicate key value violates unique constraint
    \"activation_challenges_nonce_key\"" }) })
  ```

  Unlike the local planes' SQLite tests, which each get a fresh temp-file
  database, this crate's Postgres-gated tests share one persistent container
  across every invocation — nothing truncates `activation_challenges` (or the
  other tables) between runs, and this test's nonce is fixed rather than
  randomised per call. The first run against a clean container passes; every
  run after that fails, on the same test, for a reason that has nothing to do
  with what changed.

  How this was found: validating `crates/ackplane-server/src/projection.rs`'s
  `bounded_neighborhood` LIMIT-parameter fix required running this crate's
  full Postgres-gated suite (not just the one test) against the container
  already up from the Bridge foundation work. `projection::tests::*` passed
  clean; this unrelated test failed on the second invocation.

  Fix direction (not attempted here): either randomise the nonce per test
  invocation like a real caller would, or give each test run a disposable
  schema/transaction it rolls back rather than committing against the shared
  container — the latter also protects every other Postgres-gated test in
  this crate from the same class of collision, not just this one.
