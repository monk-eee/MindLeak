### Changed

- `LedgerStore` (`crates/ackplane-server/src/ledger.rs`) now takes a clone of
  the process's single `deadpool-postgres` pool instead of holding a
  dedicated `tokio_postgres::Client` for its lifetime (ADR-0143). Its one
  transaction-holding mutation, `append`, checks out one connection and
  holds it for the whole transaction; `resolve_signing_key` checks out a
  connection per call. `append` moved from `&mut self` to `&self`, retiring
  the `Arc<Mutex<LedgerStore>>` `NodeSyncService` held solely for that
  reason. `SigningKeyError` gains a `PoolExhausted` variant so
  `LedgerStore::resolve_signing_key` (called through a generic
  `Result<KeyResolution, SigningKeyError>` bound shared with every other
  signing-key lookup) can report a pool timeout without changing that bound.
