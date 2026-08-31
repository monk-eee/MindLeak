### Changed

- `TelemetryStore` now takes the process's bounded connection pool rather
  than holding a dedicated connection for its lifetime (ADR-0143). Every
  method was already `&self` and none holds a transaction across more than
  one statement, so migration was purely mechanical: each call checks out a
  connection from the pool. Failing to obtain one within
  `ACKPLANE_DB_POOL_TIMEOUT_MS` is a new typed
  `TelemetryStoreError::PoolExhausted`, reported over gRPC as `unavailable`
  (mirroring `ClaimStore`'s ADR-0143 slice 2 mapping). `resolve_signing_key`
  now returns `TelemetryStoreError` instead of `SigningKeyError`, since
  obtaining a pooled connection is itself a new failure mode the store must
  report.
- `TelemetryGrpcService` no longer wraps its store in an `Arc<Mutex<_>>`.
  That lock was never required by mutability — every `TelemetryStore` method
  has always taken `&self` — so it only ever serialized otherwise-independent
  telemetry reads and writes for no reason; it now holds a plain
  `Arc<TelemetryStore>`.
