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
