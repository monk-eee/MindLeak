### Changed

- `SupervisorStore` now takes the process's bounded connection pool rather
  than holding a dedicated connection for its lifetime (ADR-0143). Its three
  transaction-holding mutations — `register`, `record_session`,
  `record_lifecycle` — each check out one connection and hold it for the
  whole transaction (decision 4); every read-only method (heartbeats, outbox
  positions, and `reads.rs`'s listings) checks out a connection per call.
  Failing to obtain one within `ACKPLANE_DB_POOL_TIMEOUT_MS` is a new typed
  `SupervisorStoreError::PoolExhausted`, reported over Bridge HTTP as `503
  Service Unavailable` rather than `500`.
- `register`, `record_session`, and `record_lifecycle` now take `&self`
  instead of `&mut self`, which retires the `Arc<Mutex<SupervisorStore>>`
  `NodeSyncService` held solely because those methods took `&mut self` — it
  now holds a plain `Arc<SupervisorStore>`, matching the `DirectiveStore`
  Mutex retirement already noted in `service/mod.rs`'s own doc comment.
  `service/supervisor.rs`'s frame handlers take `&SupervisorStore` instead of
  `&mut SupervisorStore` accordingly.
