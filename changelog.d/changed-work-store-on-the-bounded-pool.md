### Changed

- `WorkStore` now takes the process's bounded connection pool rather than
  holding a dedicated connection for its lifetime (ADR-0143). Its writes check
  out one connection and hold it for the whole transaction, which is
  load-bearing here: ADR-0120 decision 3 requires a Work event and its
  projection update to land in one transaction, so both must run on the same
  connection. Failing to obtain a connection within
  `ACKPLANE_DB_POOL_TIMEOUT_MS` is a typed `WorkStoreError::PoolExhausted`
  rather than a request that hangs.
- A saturated pool is reported as retryable, not as an internal fault:
  `WorkQueryService` maps it to gRPC `unavailable` (matching `ClaimStore`), and
  Work ingress maps it to its existing `Unavailable` arm.
- **This completes ADR-0143's store migration.** Every store in
  `ackplane-server` now checks out from one bounded pool per process; no
  `client: Client` field remains. `ackplane-server` and `ackplane-bridge` each
  build exactly one pool, and `ackplane-mcp` constructs no Postgres store at
  all, so the per-process cap now genuinely bounds the whole fleet's demand —
  which is what
  `gaps.d/the-postgres-connection-ceiling-fails-the-suite-misleadingly.md`
  asked for.
