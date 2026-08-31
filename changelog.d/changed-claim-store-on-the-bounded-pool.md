### Changed

- `ClaimStore` now takes the process's bounded connection pool rather than
  holding a dedicated connection for its lifetime (ADR-0143 slice 2). Its four
  CAS lease mutations — `delegate`, `release`, `renew`, `recover` — each check
  out one connection and hold it for the whole transaction, so the
  `SELECT ... FOR UPDATE` row lock they depend on stays on a stable connection
  (decision 4). Failing to obtain a connection within
  `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed `ClaimStoreError::PoolExhausted`,
  reported over gRPC as `unavailable` rather than `internal`, because it is a
  condition the caller can retry.
- Every `ClaimStore` method now takes `&self`, which retires the
  `Arc<Mutex<ClaimStore>>` that ADR-0111 introduced solely because the mutating
  methods took `&mut self`. `ClaimDelegationService`, Bridge's `AppState`, and
  the administration routes hold a plain `Arc<ClaimStore>`. Concurrent claims in
  one process are now arbitrated by the database's own CAS row lock — as they
  already were between processes — instead of being serialised by a
  process-wide lock that the pool would otherwise have made pointless.
