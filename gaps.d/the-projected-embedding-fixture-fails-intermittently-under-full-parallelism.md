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

  **Still open:** the underlying intermittent failure itself has not recurred,
  so whether it was pool exhaustion, a lock wait, or a foreign-key race remains
  unconfirmed. Re-open with the captured `ProjectionError` if this test (or any
  other `upsert_embedding` caller) fails again under `cargo test --all`.
