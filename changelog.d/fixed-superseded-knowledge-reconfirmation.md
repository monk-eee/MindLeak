- **`reconfirm` no longer refreshes a superseded knowledge statement.**
  `crates/ackplane-server/src/knowledge_store/reconfirmation.rs` guarded its
  atomic update with `retired_at IS NULL` alone, but `Superseded` deliberately
  leaves `retired_at` null, so a statement that had already been replaced could
  still have its `confirmed_at` clock refreshed and gain a new audited
  reconfirmation receipt. The guard now also excludes `lifecycle_state =
  Superseded`, while still allowing a `Candidate` statement to be reconfirmed
  (an existing, intentional behaviour). Verified against a live database via
  `ACKPLANE_TEST_DATABASE_URL` with a new record -> activate -> supersede ->
  reconfirm regression.
