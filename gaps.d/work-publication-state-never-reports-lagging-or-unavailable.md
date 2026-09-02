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
  it closed `task:1b94d6ca5365`.
  **CORRECTED 2026-09-02 on `a5771768`: the position was necessary but NOT
  sufficient, and "what remains is the comparison" is wrong.** `lagging` as
  decision 6 defines it — "the projection worker has fallen behind the
  ledger" — is unreachable by construction, because **there is no Work
  projection worker**. `crates/ackplane-server/src/projection/` covers
  `projected_nodes`, embeddings and neighbourhoods, and mentions
  `work_tasks`/`work_task_history` nowhere. Every Work write path allocates
  the position, appends the history row, and updates the projection **inside
  one transaction with the same `stream_position`**:
  `work_store/mod.rs::create_task`, `work_store/ingress.rs::record_node_task`,
  and `work_command_store/execute/mod.rs` (the `UPDATE work_tasks SET
source_event_position` at the end of the same `transaction` that inserted
  the event). So in any committed state `MAX(work_tasks.source_event_position)`
  equals `work_stream_heads.stream_position`, and a freshness comparison built
  on them can only ever answer "current". That is the broken-gauge shape this
  repository has been bitten by before: a signal that can only ever read one
  value is indistinguishable from one that is not wired up, and it would be
  shipped as decision 6 being satisfied.
  **What the comparison WOULD be good for, which is a different claim.** If a
  command path ever appended an event without its paired projection update,
  the positions would diverge — so the comparison detects a _defect_ (a
  skipped projection write), not _lag_. That is much closer to decision 3's
  replay-mismatch meaning, i.e. `unavailable`, than to `lagging`.
  **So the residual is architectural, not arithmetic, and needs a human.**
  Either Work gains a genuine asynchronous projector (ADR-0120 decision 3's
  "append-only event stream with a checked projection", which today is
  append-only and synchronously projected), or decision 6's five-state
  vocabulary is wrong for a synchronously-projected domain and should be
  amended to say so. An agent should not pick between those on its own, and
  must not implement a state that cannot occur in order to close the task.
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
