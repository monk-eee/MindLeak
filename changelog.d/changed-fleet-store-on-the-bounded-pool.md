### Changed

- `FleetStore` now takes the process's bounded connection pool rather than
  holding a dedicated connection for its lifetime (ADR-0143). Its methods were
  already `&self` (no mutating transaction ever needed `&mut`), so no `Mutex`
  or `Arc<Mutex<_>>` retirement was involved here — only `connect` itself
  changes shape, taking `&PgPool` instead of a database url, and every query
  now checks out one connection per call via a private `connection()` helper.
  Failing to obtain a connection within `ACKPLANE_DB_POOL_TIMEOUT_MS` is a
  typed `FleetStoreError::PoolExhausted`. `signing_keys::SigningKeyError`
  gains the matching `PoolExhausted` variant, since `FleetStore::timeline`/
  `signing_keys` call `signing_keys::for_repository` against the same checked-
  out connection.
