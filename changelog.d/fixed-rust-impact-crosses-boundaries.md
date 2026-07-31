- **Rust impact traversal crosses nested module and verified crate boundaries.**
  Nested grouped `use` trees now retain every module branch and alias. Imports
  through local Cargo path dependencies resolve to real workspace artifacts only
  when the nearest consumer manifest declares that dependency and its target
  manifest declares the crate root; unresolved, external, and ambiguous imports
  keep the conservative `package:<name>` fallback. Re-ingest processes manifests
  before source files so deferred artifact candidates converge regardless of
  lexical crate order.

  Measured over this repository, ingesting the same file set into a fresh graph
  with each build, `get_impact_radius` reaches further: for
  `crates/mindleak-storage/src/lib.rs`, a crate other crates consume, 90 nodes
  becomes 286. Consumers previously stopped at `package:mindleak_storage`, so a
  change there under-reported almost everything it could break. Files with fewer
  cross-crate consumers move as you would expect and no further —
  `crates/mindleak-core/src/model.rs` 287 to 300, `graph/mod.rs` 190 to 203 —
  which is the point: the new edges are the ones Cargo actually declares.
