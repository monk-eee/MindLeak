- **A lease is now a heartbeat, not a deadline (ADR-0052).** Any authenticated
  call that names a task — `task_scope`, `ask_question`, `answer`,
  `conformance_history`, `advise`, `check_conformance` — renews its lease as a
  side effect. Observed repeatedly in one session: a claim taken with a 3600s
  lease lapsed during `cargo test --all`, and the push that followed was refused
  for having no live claim, three times, while the agent was working throughout.
  Making the heartbeat free is the same shape that made question delivery
  actually adopted in ADR-0046 — a capability that depends on remembering is
  adopted at a rate of zero, so it rides on calls already being made.
  The short default lease is unchanged, because that is what frees a vanished
  agent's work quickly; renewal-on-activity keeps that property instead of
  trading it away by raising the default. A heartbeat can only extend a lease,
  never shorten one an owner deliberately took long, and it leaves
  `claim_started_at` alone so the evidence window still bounds exactly what the
  claim covered (ADR-0048 is unaffected). It is owner-only and silent: a peer
  reading the task renews nothing, an already-lapsed lease is not resurrected by
  a passing call, and neither case errors — the call it rides on has its own job
  to do, and a lapse must still require a deliberate re-claim rather than
  undoing a claim someone else has taken.
