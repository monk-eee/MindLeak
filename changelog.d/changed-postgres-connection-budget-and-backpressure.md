### Changed

- Every store's `connection()` helper now checks a connection out through a
  new `db_pool::checkout` (`crates/ackplane-server/src/db_pool.rs`) instead of
  calling `pool.get()` directly. Once as many callers are already queued as
  the pool has slots, `checkout` refuses immediately with the identical
  `PoolExhausted` a caller joining that queue would eventually get anyway,
  rather than waiting the full configured `ACKPLANE_DB_POOL_TIMEOUT_MS` to
  learn the same thing every caller ahead of it has already learned. No
  store's error type, `#[from]` conversion, or `SERVICE_UNAVAILABLE`/
  `Status::unavailable` mapping needed to change: `checkout` returns the same
  `deadpool_postgres::PoolError` type `pool.get()` already did.
- Added [`docs/POSTGRES-CONNECTION-BUDGET.md`](../docs/POSTGRES-CONNECTION-BUDGET.md),
  a justified `ACKPLANE_DB_POOL_MAX_SIZE` sizing recommendation for a
  production deployment topology, building on ADR-0143's now-complete bounded
  pool. Closes the residual left by
  `gaps.d/the-postgres-connection-ceiling-fails-the-suite-misleadingly.md`.
