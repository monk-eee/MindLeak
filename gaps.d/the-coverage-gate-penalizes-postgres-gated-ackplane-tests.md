- **The `Coverage (Rust + VS Code)` CI gate (`cargo llvm-cov report
  --fail-under-lines 80`) has no Postgres service, so every ackplane-server
  store's real, Postgres-gated tests hollow-skip in that job specifically --
  and the cumulative effect of many Ackplane Postgres-backed features
  landing this session (Knowledge, Constitution, Readiness, stranded-claims,
  and more) has pushed the aggregate line percentage right to the 80% edge.**
  Confirmed on PR #585 (`feat/bridge-readiness-rollup`): its own
  `readiness.rs` is a `tokio_postgres`-backed store shaped identically to
  `constitution_store.rs`/`knowledge_store.rs` (which already show ~12%
  coverage in this same job for the same reason), not a file that shipped
  without real tests. A genuine, root-cause fix landed the same day
  (task:22f9d48b8414, PR #590: `worktree_roots` in
  `mindleak-storage/src/repository/worktree.rs` had shipped with zero
  coverage from an earlier split-repository.rs refactor, dragging lines to
  79.87%) -- but #585's own re-run afterward still failed at 79.76% lines,
  because its new Postgres-gated code added more uncovered lines than the
  worktree.rs fix recovered. `.github/workflows/ci.yml`'s `coverage` job
  provisions no `services:` block at all, so this is not specific to one
  file; it is structural.

  Impact: any future PR touching an Ackplane Postgres-backed store can flip
  this gate red through no fault of its own diff, and the failure looks
  identical to a real coverage regression -- costly to re-diagnose each time
  without checking whether the failing files are Postgres-gated stores
  first (`git grep -l tokio_postgres::Client crates/ackplane-server/src`).

  Not fixed this run: this needs an explicit design decision, not a
  drive-by CI edit -- either (a) add a real Postgres service container to
  the `coverage` job so these tests execute and get counted honestly
  (bigger blast radius: also changes what the job needs to provision, and
  ADR-0088 deliberately keeps other jobs Postgres-free), or (b) exclude
  Ackplane's Postgres-gated store files from the `--fail-under-lines` gate
  specifically via `cargo llvm-cov`'s `--ignore-filename-regex` (narrower,
  but risks quietly hiding a REAL future coverage regression inside an
  excluded file if the pattern is too broad). Whichever is chosen belongs
  in an ADR given how visibly it changes what "80% coverage" is measured
  against.
