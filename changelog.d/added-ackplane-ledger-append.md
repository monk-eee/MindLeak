- **Added the Ackplane ledger schema and its idempotent append transaction
  (ADR-0086).** `ackplane-server` gained a `ledger` module: `stream_heads`,
  `ledger_records`, and `ledger_receipts` tables (applied idempotently from
  `migrations/0001_ledger.sql`), and `LedgerStore::append` — one transaction
  that locks the destination stream's head, checks the ADR-0083 deduplication
  key `(tenant_id, repository_id, producer_id, producer_sequence)`, appends at
  most one record, writes its receipt, and advances the head. A same-key/
  same-digest retry returns the stored position without appending a second
  row; a same-key/different-digest retry is refused as a non-retryable
  conflict. Tests that need a real PostgreSQL connection are opt-in via
  `ACKPLANE_TEST_DATABASE_URL` and skip (rather than fail or hang) when it is
  unset, so `cargo test --workspace` keeps passing on a machine with no
  container runtime, PostgreSQL, or network (ADR-0088 clause 2).
