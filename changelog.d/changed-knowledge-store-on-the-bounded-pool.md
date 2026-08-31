### Changed

- `KnowledgeStore` now takes a clone of the process's bounded connection pool
  rather than holding a dedicated `tokio_postgres::Client` for its lifetime
  (ADR-0143 slice 5). Every method checks out one connection per call via a
  private `connection()` helper and reuses it for every statement inside that
  call (`record`'s two inserts, `activate`'s guarded update plus its fallback
  diagnostic select, `supersede`'s guarded update plus its own diagnostic
  fallback) — no method here opens an explicit multi-statement transaction, so
  no special handling beyond "one checkout per call" is needed (decision 4).
  Failing to obtain a connection within `ACKPLANE_DB_POOL_TIMEOUT_MS` is a
  typed `KnowledgeStoreError::PoolExhausted`, reported over gRPC as
  `unavailable` and over Bridge's HTTP API as `503 Service Unavailable` rather
  than `500`, because it is a condition the caller can retry, mirroring
  `ClaimStore`'s own mapping.
- `ackplane-server`'s and `ackplane-bridge`'s `main.rs` construct
  `KnowledgeStore` from the process's already-built shared pool instead of a
  raw `database_url`. Test fixtures across `ackplane-server` and
  `ackplane-bridge` build one pool per test and pass it to `KnowledgeStore::
  connect`, closing the same unbounded per-test connection demand this
  migration already closed for `LiveFeedStore` and `ClaimStore`.
