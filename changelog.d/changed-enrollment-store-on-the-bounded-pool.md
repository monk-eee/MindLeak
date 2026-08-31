### Changed

- `EnrollmentStore` (`crates/ackplane-server/src/enrollment_store/{mod,activation,rotation,status_check,submission}.rs`)
  now takes a clone of the process's single `deadpool-postgres` pool instead
  of holding a dedicated `tokio_postgres::Client` for its lifetime (ADR-0143).
  Its five transaction-holding mutations -- `submit`, `approve` (submission.rs),
  `issue_challenge`, `activate` (activation.rs), and `rotate_key` (rotation.rs)
  -- each check out one connection and hold it for their whole transaction;
  `find_binding` and `consume_status_nonce` (status_check.rs) check out a
  connection per call. All five held-transaction methods moved from `&mut self`
  to `&self`, retiring the `Arc<Mutex<EnrollmentStore>>` `NodeEnrollmentService`
  held solely for that reason.
