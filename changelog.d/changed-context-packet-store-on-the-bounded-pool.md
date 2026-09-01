### Changed

- `ContextPacketStore` now takes the process's bounded connection pool rather
  than holding a dedicated connection for its lifetime (ADR-0143 slice 4).
  `store_packet` checks out one connection and holds it for its whole
  transaction (decision 4); `get_packet`, `record_use`, `list_use_receipts`,
  and `list_packet_summaries` each check out a connection per call. Failing to
  obtain a connection within `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed
  `ContextPacketStoreError::PoolExhausted`, reported over Bridge HTTP as `503
  Service Unavailable` rather than `500`, because it is a condition the caller
  can retry.
- `store_packet` and `record_use` now take `&self` instead of `&mut self`:
  neither needs to hold a private connection any more, so nothing about the
  store's own state changes when it writes.
