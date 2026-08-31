### Changed

- `Projector` now takes a clone of the process's bounded connection pool
  rather than holding a dedicated `tokio_postgres::Client` for its lifetime
  (ADR-0143 slice 7). `rebuild`'s held transaction checks out one connection
  for the life of the transaction; every other method (`bounded_neighborhood`,
  `sample_nodes`, `stale_projections`, `freshness`, `nodes_missing_embedding`,
  `upsert_embedding`) checks out one connection per call. Since no method any
  longer needs a persistent `&mut Client`, `rebuild`, `rebuild_stale`, and
  their private helper changed from `&mut self` to `&self`. Failing to obtain
  a connection within `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed
  `ProjectionError::PoolExhausted`.
- `ackplane-server`'s `main.rs`/`schema_migration.rs` and `ackplane-bridge`'s
  `main.rs` construct `Projector` from the process's already-built shared pool
  instead of a raw `database_url`. Test fixtures across `ackplane-server` and
  `ackplane-bridge` build one pool per test and pass it to `Projector::connect`.
