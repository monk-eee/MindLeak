### Changed

- `ReadinessStore` now takes a clone of the process's bounded connection pool
  rather than holding a dedicated `tokio_postgres::Client` for its lifetime
  (ADR-0143 slice 6). Its only method, `readiness`, checks out one connection
  per call and reuses it for both of its queries and its per-signing-key
  `signing_keys::for_repository` lookup -- it opens no explicit
  multi-statement transaction, so no special held-connection handling was
  needed. Failing to obtain a connection within `ACKPLANE_DB_POOL_TIMEOUT_MS`
  is a typed `ReadinessError::PoolExhausted`.
- `ackplane-bridge`'s `main.rs` constructs `ReadinessStore` from the process's
  already-built shared pool instead of a raw `database_url`. Test fixtures in
  `ackplane-server` and `ackplane-bridge` build one pool per test and pass it
  to `ReadinessStore::connect`.
