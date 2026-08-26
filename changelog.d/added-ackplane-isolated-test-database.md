- **Added:** `docker-compose.yml` now provisions a separate `ackplane_test`
  database (`test-db-init`, `migrate-test`) alongside the `ackplane` database
  the long-running `ackplane` service's projection worker operates against.
  A test asserting a projection is *behind* — `readiness::tests::
  readiness_needs_attention_when_the_projection_is_lagging`
  ([`crates/ackplane-server/src/readiness.rs`](crates/ackplane-server/src/readiness.rs))
  — only holds while nothing else is catching the ledger up, and the dev
  stack's own worker does exactly that when pointed at the same database:
  measured 491 passed with the stack down, then 490 passed and this one
  failed at the pre-push hook with the stack up, same commit, minutes apart.
  `ACKPLANE_TEST_DATABASE_URL` should now point at `ackplane_test` (see
  `.env.example`) so local `cargo test` runs never share storage with a live
  worker. See ADR-0133.
