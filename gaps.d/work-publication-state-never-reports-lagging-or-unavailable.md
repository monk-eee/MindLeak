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
  **Blocked, not merely unwritten — measured 2026-09-01 on `e9a4cd6f`.** The
  paragraph above names a "last-applied ledger position". There is no such
  position: `work_tasks` has no `source_event_position`, `work_task_history`
  has no `stream_position`, and `grep position
  crates/ackplane-server/migrations/*work*.sql` returns nothing at all —
  history is ordered by `recorded_at`, not by an allocated stream position.
  ADR-0120 decision 3's authoritative ordered event log is what would supply
  one, and that is the still-`blocked` task `task:1b94d6ca5365` ("Industrial
  Work foundation: append-only task creation and checked projection").
  So `lagging` cannot be computed today by any means short of inventing a
  position column, which is that foundation's job and not something a
  publication-state fix should quietly annex; and `unavailable` — decision 3's
  replay-mismatch repair state — has the identical dependency, since replaying
  events requires an event log to replay. Recorded here because the wording
  above reads as though the check were merely unwritten, which costs whoever
  picks this up the same investigation twice. This stays a gap rather than a
  known limitation: the job is real and fixable here, it just cannot start
  until its dependency lands.
