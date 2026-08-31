### Changed

- `DesignStore` now takes a clone of the process's bounded connection pool
  rather than holding a dedicated `tokio_postgres::Client` for its lifetime
  (ADR-0143 slice 8). `create_design` and `record_decision` each check out one
  connection for the life of their transaction; `get_design`, `list_decisions`,
  and `list_designs` each check out one connection per call. Since neither
  mutation needs a persistent `&mut Client` any more, both changed from
  `&mut self` to `&self`. Failing to obtain a connection within
  `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed `DesignStoreError::PoolExhausted`.
- `ackplane-bridge`'s Design API retires its `designs: Arc<Mutex<DesignStore>>`
  field to a plain `Arc<DesignStore>` -- the `Mutex` existed only because
  `create_design`/`record_decision` needed `&mut self`. Its sibling
  `materializations: Arc<Mutex<MaterializationStore>>` field is unchanged
  (`MaterializationStore` is a separate store, migrated in its own slice).
  `main.rs` constructs `DesignStore` from the process's already-built shared
  pool instead of a raw `database_url`. Test fixtures across `ackplane-server`
  and `ackplane-bridge` build one pool per test and pass it to
  `DesignStore::connect`.
