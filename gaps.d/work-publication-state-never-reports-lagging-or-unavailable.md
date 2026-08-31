- **Neither Work read surface computes ADR-0120 decision 6's `lagging` or
  `unavailable` publication states — MEASURED 2026-08-30, OPEN.** Decision 6
  names five publication states a Work answer must carry: `current`,
  `lagging`, `claims_only`, `not_published`, or `unavailable`. Bridge's own
  `WorkPublicationResponse::from` (`crates/ackplane-bridge/src/work_api.rs`)
  only ever computes three of them (`current`/`claims_only`/`not_published`);
  the new `ackplane-mcp` `task_query` tool and its `WorkQueryService`
  backend (`crates/ackplane-server/src/work_query_service.rs`, ADR-0139
  clause 2) intentionally match that exact mapping rather than getting ahead
  of it, since the ADR's instruction was to expose the projection "exactly
  as Bridge's first Work read surface already does". `lagging` (the
  projection worker has fallen behind the ledger) and `unavailable`
  (decision 3's replay-mismatch repair state) are not computed anywhere in
  this codebase today.
  **What is actually needed:** a projection-freshness check — comparing the
  Work projection's last-applied ledger position against the ledger's
  current position, and a replay-consistency check per decision 3 — added to
  `WorkStore::publication` (or a sibling method), then threaded through both
  Bridge's `WorkPublicationResponse` and `WorkQueryService`'s
  `WorkPublicationSummary` together so the two surfaces do not drift apart
  again. Out of scope for ADR-0139 clause 2 alone: it is a `WorkStore`/
  projection-worker change shared by both read surfaces, not an
  `ackplane-mcp`-only fix.
