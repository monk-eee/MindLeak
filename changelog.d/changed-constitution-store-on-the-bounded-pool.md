### Changed

- `ConstitutionStore` now takes the process's bounded connection pool rather
  than holding a dedicated connection for its lifetime (ADR-0143). Its three
  transaction-holding mutations -- `publish`, `propose_clause`,
  `record_publication` -- each check out one connection and hold it for the
  whole transaction (decision 4); read-only methods (`get_active`,
  `resolve_signing_key`, `consume_constitution_nonce`, `list_proposals`,
  `withdraw_proposal`, `get_publication`, `list_publications`) each check out
  a connection per call. Failing to obtain a connection within
  `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed
  `ConstitutionStoreError::PoolExhausted`, reported over gRPC as
  `unavailable` (mirroring `ClaimStore`'s ADR-0143 slice 2 mapping) and over
  Bridge HTTP as `503 Service Unavailable` rather than `500`.
- `publish`, `propose_clause`, and `record_publication` now take `&self`
  instead of `&mut self`, which retires the `Arc<Mutex<ConstitutionStore>>`
  ADR-0126 introduced solely because those methods took `&mut self` --
  `ConstitutionGrpcService` and Bridge's `AppState` now hold a plain
  `Arc<ConstitutionStore>`, the same change ADR-0143 slice 2 already made for
  `ClaimStore`.
