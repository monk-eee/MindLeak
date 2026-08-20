### Added

- `EnrollmentActivationResult` (the `ActivateEnrollment` RPC's response)
  carries the server-assigned `signing_key_id`. Every real client of
  `NodeSyncService.Synchronize` needs this id to send `Hello`, and until now
  the only way to learn it was a direct database read no external node could
  actually perform. `crates/ackplane-client/examples/enroll_and_sync.rs` and
  its integration test now read it straight from the RPC response instead of
  querying `signing_keys` directly.
