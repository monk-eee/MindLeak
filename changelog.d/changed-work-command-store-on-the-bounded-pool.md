### Changed

- `WorkCommandStore` now takes the process's bounded connection pool rather
  than holding a dedicated connection for its lifetime (ADR-0143). Every write
  checks out one connection and holds it for the whole transaction — which
  matters more here than in most stores: a supervisor-directed command writes
  its Work/Claim effect and issues its ADR-0107 directive on one transaction,
  so both must land on the same connection or neither is atomic. Failing to
  obtain a connection within `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed
  `WorkCommandStoreError::PoolExhausted` rather than a request that hangs.
- `WorkCommandService::connect` takes the pool too, and every method now takes
  `&self`. That retires the `Arc<Mutex<WorkCommandService>>` Bridge's Work
  command routes held — a process-wide lock that existed only because
  `Client::transaction()` needs `&mut`, and which serialised every submit and
  confirm across all callers while guarding nothing the database's own
  transaction was not already guarding. `WorkCommandApiState` holds a plain
  `Arc<WorkCommandService>`.
