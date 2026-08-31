### Changed

- `AdministrationStore` now takes a clone of the process's bounded connection
  pool rather than holding a dedicated `tokio_postgres::Client` for its
  lifetime (ADR-0143 slice 9). Every held transaction (adopted-policy
  adoption, Snapshot request/receipt, Lifecycle-purge preview/confirm, Export
  request/receipt, and recovery-execution preview/confirm) checks out one
  connection for the life of the transaction; every read-only method
  (including Recovery inspection/rehearsal persistence, which issues single
  statements with no explicit transaction) checks out one connection per
  call. Since no method needs a persistent `&mut Client` any more, every
  method changed from `&mut self` to `&self`. Failing to obtain a connection
  within `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed
  `AdministrationStoreError::PoolExhausted`, mapped to HTTP 503 in
  `ackplane-bridge`'s Administration routes.
- `ackplane-bridge`'s Administration API retires its
  `administration: Arc<Mutex<AdministrationStore>>` field to a plain
  `Arc<AdministrationStore>` -- the `Mutex` existed only because every method
  needed `&mut self`. `main.rs` constructs `AdministrationStore` from the
  process's already-built shared pool instead of a raw `database_url`. Test
  fixtures across `ackplane-server` and `ackplane-bridge` build one pool per
  test and pass it to `AdministrationStore::connect`.
