### Added

- `ackplane-client` ships a real, tested enroll-and-sync example
  (`examples/enroll_and_sync.rs`) and a Postgres-gated integration test
  proving the full node lifecycle end to end over genuine gRPC: submit an
  enrollment request, complete activation with a real Ed25519 proof of
  possession, and publish one real signed event through
  `NodeSyncService.Synchronize` -- enough for a real repository to appear in
  the Bridge Fleet view (ADR-0095) for the first time.
