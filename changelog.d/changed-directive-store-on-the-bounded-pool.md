### Changed

- `DirectiveStore` now takes the process's bounded connection pool rather than
  holding a dedicated connection for its lifetime (ADR-0143 slice 4). Its two
  writes — `enqueue` and `record_receipt` — each check out one connection and
  hold it for the whole transaction, so the `FOR KEY SHARE` row lock
  `record_receipt` depends on stays on a stable connection (decision 4);
  `pending_for_session` checks one out per read and returns it immediately.
  Failing to obtain a connection within `ACKPLANE_DB_POOL_TIMEOUT_MS` is a
  typed `DirectiveStoreError::PoolExhausted` rather than a request that hangs.
- Every `DirectiveStore` method now takes `&self`, which retires the
  `Arc<Mutex<DirectiveStore>>` that `NodeSyncService` held solely because the
  writes took `&mut self`. Directive enqueueing and receipt recording were
  previously serialised across every connected supervisor by that
  process-wide lock; they are now arbitrated by the database's own
  transaction, as they already were between processes.
