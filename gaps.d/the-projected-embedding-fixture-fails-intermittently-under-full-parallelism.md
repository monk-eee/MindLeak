- **`projection::embeddings::tests::the_candidate_set_is_bounded_by_the_limit`
  fails intermittently under `cargo test --all`, in its fixture rather than its
  assertion — OBSERVED ONCE 2026-09-02 on `2437b4ec`, NOT REPRODUCED, OPEN.**
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

  **What was not captured, and what the next person should capture:** the
  underlying `tokio_postgres` error. The panic message was truncated to the
  `expect` string in the run that caught it, so whether this was pool exhaustion
  (`TEST_POOL_MAX_SIZE` under whole-workspace parallelism), a lock wait, or a
  foreign-key race against `projected_nodes` is unknown. Changing that `expect`
  to report the error itself would make a single future occurrence conclusive
  instead of merely suggestive — the fixture currently discards exactly the
  information needed to diagnose it.

  **Impact:** low but corrosive — an intermittent red in a suite that is
  otherwise reliably green teaches agents to re-run rather than investigate, and
  this one names a test whose title ("bounded by the limit") points away from
  where it actually failed.

  **Not fixed this run:** observed while landing ADR-0140 slice 3a, which
  touches neither this test nor `upsert_embedding`; recorded rather than
  silently re-run.
