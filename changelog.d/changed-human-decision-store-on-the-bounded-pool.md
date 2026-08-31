### Changed

- `HumanDecisionStore` now takes the process's bounded connection pool rather
  than holding a dedicated connection for its lifetime (ADR-0143). Its two
  writes — `request` and `resolve` — each check out one connection and hold it
  for the whole transaction, so the ADR-0115 stream lock they depend on stays
  on a stable connection (decision 4); `get` and `list_page` check one out per
  read and return it immediately. Failing to obtain a connection within
  `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed
  `HumanDecisionStoreError::PoolExhausted` rather than a request that hangs.
- Both write methods now take `&self`. No lock is retired here: Bridge already
  held this store as a plain `Arc<HumanDecisionStore>`, and its `&mut`
  requirement had been satisfied by constructing it per use rather than by a
  `Mutex`.
