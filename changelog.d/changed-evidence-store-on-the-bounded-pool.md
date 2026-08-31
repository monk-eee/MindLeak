### Changed

- `EvidenceStore` now takes a clone of the process's bounded connection pool
  rather than holding a dedicated `tokio_postgres::Client` for its lifetime
  (ADR-0143). Every method (record, list, detail, conformance record/list/
  detail) checks out one connection per call, reusing a single checkout
  within `record_conformance`'s two sequential statements. Failing to obtain
  a connection within `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed
  `EvidenceStoreError::PoolExhausted`/`ConformanceStoreError::PoolExhausted`,
  mapped to gRPC `unavailable` in `evidence_service.rs` (mirroring
  `ClaimStore`'s mapping) and to HTTP 503 in `ackplane-bridge`'s
  `evidence_api.rs`.
- `resolve_signing_key` now returns `EvidenceStoreError` rather than
  `SigningKeyError`, matching `ClaimStore`'s precedent: obtaining a pooled
  connection is itself a failure mode the store must report.
- `evidence_service.rs`'s `EvidenceGrpcService` retires its
  `Arc<Mutex<EvidenceStore>>` in favor of a plain `Arc<EvidenceStore>` --
  every method was already `&self`, so the `Mutex` was never justified by
  mutability. `ackplane-server`'s `main.rs`/`schema_migration.rs` and
  `ackplane-bridge`'s `main.rs`/`evidence.rs` (`BridgeEvidenceStore`)
  construct `EvidenceStore` from the process's already-built shared pool
  instead of a raw `database_url`. Test fixtures across `ackplane-server` and
  `ackplane-bridge` build one pool per test and pass it to
  `EvidenceStore::connect`.
