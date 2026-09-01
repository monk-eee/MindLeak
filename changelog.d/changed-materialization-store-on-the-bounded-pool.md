### Changed

- ADR-0143: `MaterializationStore` (Industrial design materialization revisions,
  `crates/ackplane-server/src/design_materialization_store.rs`) now takes a clone
  of the process's single bounded `deadpool_postgres` pool instead of holding a
  dedicated `tokio_postgres::Client` for its lifetime. `record_materialization`
  moved from `&mut self` to `&self`, retiring the Bridge's
  `Arc<Mutex<MaterializationStore>>` in favor of a plain `Arc<MaterializationStore>`.
  A pool-checkout timeout now surfaces as `MaterializationStoreError::PoolExhausted`,
  mapped to HTTP 503 at the Bridge layer.
