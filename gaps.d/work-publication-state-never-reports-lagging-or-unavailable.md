- **Neither Work read surface computes ADR-0120 decision 6's `lagging` or
  `unavailable` publication states — MEASURED 2026-08-30, OPEN, and BLOCKED on
  the Work event-log foundation (`task:1b94d6ca5365`).** Decision 6
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
  **No longer blocked — the foundation landed.** `work_task_history` now
  carries an allocated, repository-scoped `stream_position` and `work_tasks`
  carries the `source_event_position` it was projected from (migration
  `0065_work_event_positions.sql`, allocated per `(tenant, repository)` from
  `work_stream_heads`). That was the missing dependency described below, and
  it closed `task:1b94d6ca5365`. What remains is the original job: compute the
  freshness comparison and thread it through both read surfaces together.
  **Why it was blocked, kept for whoever picks this up — measured 2026-09-01
  on `e9a4cd6f`.** The paragraph above names a "last-applied ledger position".
  There was no such position: `work_tasks` had no `source_event_position`,
  `work_task_history` had no `stream_position`, and `grep position
  crates/ackplane-server/migrations/*work*.sql` returned nothing at all —
  history was ordered by `recorded_at`, not by an allocated stream position.
  So `lagging` could not be computed by any means short of inventing a
  position column, which was that foundation's job and not something a
  publication-state fix should quietly annex; and `unavailable` — decision 3's
  replay-mismatch repair state — had the identical dependency, since replaying
  events requires an event log to replay. This stays a gap rather than a known
  limitation: the job is real and fixable here, and now unblocked.
