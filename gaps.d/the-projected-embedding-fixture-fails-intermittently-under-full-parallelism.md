- **`projection::embeddings::tests::the_candidate_set_is_bounded_by_the_limit`
  fails intermittently under `cargo test --all`, in its fixture rather than its
  assertion — OBSERVED ONCE 2026-09-02 on `2437b4ec`, NOT REPRODUCED, diagnostics
  now fixed (2026-09-02), root cause still OPEN if it recurs.**
  A full `cargo test --all` against a real PostgreSQL reported
  `650 passed; 1 failed`, the failure being a panic at
  `crates/ackplane-server/src/projection/embeddings.rs:402` — the
  `.expect("embedding is accepted")` inside `ranked_fixture`, not the
  bounded-limit assertion the test is named for. So the test did not observe a
  wrong answer; its `upsert_embedding` call failed to complete.

  **Why it looks like parallel-load flakiness rather than a real defect:**
  `ranked_fixture` is shared with `candidates_are_ranked_by_cosine_distance`,
  which passed in the same run. Re-running the module alone
  (`cargo test -p ackplane-server projection::embeddings`) passed 9 of 9, and a
  full `cargo test -p ackplane-server --lib` passed 651 of 651. Only
  `--all` — which adds the other crates' database-backed suites against the same
  database concurrently — has produced it, once.

  **Fixed 2026-09-02:** every `upsert_embedding(...).expect("...")` call in this
  file's test module (6 call sites, including `ranked_fixture`'s loop) now reads
  `.unwrap_or_else(|e| panic!("upsert_embedding failed for {node_id}: {e}"))` or
  the same shape with the relevant node/repo identifiers, so a future occurrence
  reports the actual `ProjectionError` (pool exhaustion vs. deadlock vs. a
  foreign-key race) instead of a bare "embedding is accepted" message. This is
  diagnostics only -- no production code changed, and all 9 tests in the module
  still pass unchanged.

  **Impact:** low but corrosive — an intermittent red in a suite that is
  otherwise reliably green teaches agents to re-run rather than investigate, and
  this one names a test whose title ("bounded by the limit") points away from
  where it actually failed.

  **Recurrence 2026-09-14, still OPEN:** PR #948 queue candidate
  `d789753ad9518b86f49beaa344361554ca3367d8`, Industrial CI run `34791453133`,
  failed `projection::tests::an_embedding_can_reference_an_existing_projected_node`
  in `crates/ackplane-server/src/projection/mod.rs` before its round-trip
  assertion. PostgreSQL reported SQLSTATE `23503`: the tenant/repository/node
  was not present when the embedding INSERT checked its foreign key. The fixture
  appends a real ledger fact and calls `Projector::rebuild`; it does not fabricate
  a projected node directly. This identifies the failure class, not the exact
  concurrent cause. The unchanged candidate passed one failed-job rerun and
  PR #948 merged on 2026-09-14; no assertion, constraint or database gate was
  weakened. Root cause remains
  unresolved and must not be treated as a clean readiness qualification.

  **Related production defect reproduced and fixed 2026-09-14:**
  `projection::rebuild::concurrency::a_delayed_stale_scan_does_not_discard_newly_indexed_vectors`
  pauses `Projector::rebuild_stale` inside its initial scan using a private
  connection-local PostgreSQL view and advisory barrier. Another projector then
  catches up the same repository and stores its vector. Resuming the original
  scan previously repeated the full rebuild and deleted that newly stored vector.
  The assertion failed before the fix and passed afterward. Rebuilds now
  serialize by tenant/repository; background work rechecks the checkpoint inside
  that transaction before deleting anything. A redundant pass preserves vectors
  and the original projection timestamp and reports zero actual rebuilds.
  Separate tests prove unrelated repositories can progress and explicit rebuilds
  still invalidate derived vectors. The 40-test projection slice passed with
  normal parallel execution against a disposable database.

  **OPEN residual:** this proves stale-scan vector loss, not the exact cause of
  the earlier SQLSTATE `23503` failure. That fixture's foreign-key failure was
  not reproduced by the controlled interleaving. Preserve the earlier evidence;
  do not claim that passing this regression certifies the intermittent CI issue
  or the separately running endurance candidate.

  **Schema-fixture failure reproduced and fixed 2026-09-14:**
  `projection::embedding_concurrency_tests::embedding_writers_handle_an_atomic_node_replacement_without_foreign_key_errors`
  holds an atomic projected-node delete/reinsert open until PostgreSQL reports
  that the existing schema fixture's INSERT is blocked behind it. Committing
  the replacement produced SQLSTATE `23503` in that unchanged raw INSERT,
  although the same node key exists again after commit. The regression failed
  before and passed after the schema fixture acquired the existing
  tenant/repository rebuild lock in a ReadCommitted transaction spanning its
  INSERT and readback. No production SQL, constraints, or global test
  concurrency changed.

  The same controlled interleaving proves the public `upsert_embedding` already
  handles replacement: its conditional source-row lock returns `false`, not a
  foreign-key error; reading the current source and publishing it succeeds.
  The raw schema fixture now shares one tested implementation with its
  concurrency regression. This reproduces and removes a concrete cause of the
  2026-09-14 fixture failure class, not a trace of the original CI interleaving.
  Preserve the 2026-09-02 failure and historical CI evidence: that older
  `upsert_embedding` call's exact failure was never captured. The active
  `dcc3f289` endurance candidate is unchanged and does not include this fixture
  correction.
