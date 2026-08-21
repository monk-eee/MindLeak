- **Bridge's Fleet UI cannot list a stranded (lease-expired) claim, only
  recover one by a task id the operator already knows.** ADR-0111's own
  context assumed "an operator can already see... when its lease expires"
  from the existing `GET /api/v1/repositories/:repository_id/claims` view —
  but that route is backed by `FleetStore::active_work`, whose SQL filters
  `lease_expires_at > now` specifically because its own doc comment states
  expired claims are "not current work." The moment a claim's lease actually
  expires, it silently disappears from the only list Bridge renders, which
  is exactly the claim `recover` exists to act on.

  Impact: the new recovery form (ADR-0111) works correctly once given a
  task id, but an operator has no in-Bridge way to discover which task ids
  are currently stranded — they still need Lodestar's own `view=overlap`/
  board, or Ackplane's raw ledger, to find one. `FleetStore::claim_owner`
  (added alongside `recover`) proves the read is easy to add; nothing yet
  exposes a *list* of expired-but-undelegated claims to the UI.

  Not fixed this run: deliberately narrower than ADR-0111's own decision,
  which scoped this change to "the route... the UI control" (singular),
  not a new stranded-claims list endpoint. A real fix needs its own design
  call (extend `active_work`'s contract vs. add a second, narrower read)
  rather than silently redefining an existing, tested method's meaning as a
  side effect of adding `recover`.
