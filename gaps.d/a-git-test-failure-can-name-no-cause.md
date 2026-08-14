- **Tests that spawn `git` fail with an empty reason when the machine is under
  memory pressure, so a resource problem reads as a code problem.** Observed
  2026-08-14 on a `cargo test --all` run while the agent fleet was building
  concurrently: six `merge_tests` failed, and the panic messages were
  `git ["commit", "-m", "base"] failed:` and
  `git ["init", "--initial-branch=main"] failed:` with nothing after the colon.
  Only one of the six happened to surface the real cause —
  `fatal: Out of memory, malloc failed (tried to allocate 1048576 bytes)`. All
  eleven passed immediately when re-run with `--test-threads=1`.
  The harness at
  [`crates/lodestar-core/src/merge_tests.rs`](../crates/lodestar-core/src/merge_tests.rs)
  panics with the command and git's stderr, but git writes nothing to stderr for
  some of these failures, so the message carries no cause at all. Impact is
  wasted diagnosis on a green codebase, and worse, a plausible-looking reason to
  suspect the change under test: the failing names (`merge_evidence_*`) are
  unrelated to whatever the author was editing, which is exactly when a
  developer starts doubting their own work. The fix is for the harness to
  include git's exit status and stdout alongside stderr, and to say when stderr
  was empty rather than printing a bare colon. Left for later; nothing is wrong
  with the tests themselves.
