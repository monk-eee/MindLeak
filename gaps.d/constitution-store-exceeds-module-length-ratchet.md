- **What**: `crates/ackplane-server/src/constitution_store.rs` is 521
  non-test lines, over the 450-line module-length ratchet that
  `scripts/measure-module-length.mjs` reports against.
- **Where**: `crates/ackplane-server/src/constitution_store.rs`, grown by
  `65021bab` (ADR-0106 decision 3) and `855e6d29` (ADR-0121 decision 1,
  Constitution publication history).
- **Impact**: none yet -- `measure-module-length.mjs` is observational
  (exits 0 even over the line), so nothing is blocked. Left unaddressed,
  the file keeps growing past the point a `mod.rs` split stays easy.
- **Not fixed this run**: observed while validating an unrelated merge
  (`feat/industrial-work-board` merging `origin/main`); this branch never
  touches `constitution_store.rs` (confirmed via `git diff
  <merge-base>..HEAD -- crates/ackplane-server/src/constitution_store.rs`
  is empty), so splitting it here would be out of scope for that change.
  Split it the same way `work_store.rs` was split this week -- likely along
  publication-history vs core-store lines -- next time that file is touched
  for a real feature.
