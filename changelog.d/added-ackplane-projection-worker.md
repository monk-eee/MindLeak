### Added

- `ackplane-server` runs a background projection worker (ADR-0086 clause 9):
  on a configurable interval (`ACKPLANE_PROJECTION_INTERVAL_SECS`, default 5
  seconds) it discovers every repository whose committed structural facts are
  ahead of their projection checkpoint and rebuilds them, so an enrolled
  repository's graph projection actually gets built instead of sitting in the
  `Never projected` state forever. `crates/ackplane-client/examples/enroll_and_sync.rs`
  now publishes a genuine structural fact (not an opaque payload), so a real
  run demonstrates the Bridge Fleet view moving from `Never projected` to a
  real projected position.
