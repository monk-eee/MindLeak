### Changed

- `DelegationStore` now takes the process's bounded connection pool rather
  than holding a dedicated connection for its lifetime (ADR-0143 slice 3).
  `grant` and `revoke` each check out one connection and hold it for the
  whole transaction, so the `SELECT ... FOR UPDATE` row lock the event stream
  depends on stays on a stable connection (decision 4); `authorize_use`
  holds its own checked-out connection for the same reason. Failing to
  obtain a connection within `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed
  `DelegationStoreError::PoolExhausted` / `DelegationUseError` pool variant
  rather than an unbounded wait.
- Every `DelegationStore` method now takes `&self`. `ackplane-bridge`'s
  `AppState` already held a plain `Arc<DelegationStore>`, so no caller
  needed to change beyond passing the pool at construction time.
