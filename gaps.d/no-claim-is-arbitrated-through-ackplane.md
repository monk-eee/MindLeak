- **Claim ownership (`claim`/`renew`/`release`/`recover`) and overlap detection
  now both route through Ackplane for a federated repository — NARROWED
  2026-08-18, residual left OPEN.** `Lodestar::with_federated_claim_authority`
  (ADR-0096 clauses 2-4, 6) and `Lodestar::with_federated_claim_source`
  (clause 5) are both real, tested seams; `lodestar-mcp`'s `federation.rs`
  wires an authenticated `ackplane-client` implementation of the former
  behind the `federation-client` cargo feature, proven end-to-end against a
  real `ackplane-server` and Postgres. Every ownership-affecting call — claim,
  renew, release, recover, and the overlap pre-flight — that this repository
  exposes today is covered.

  What remains outside the wire contract: `park` (a task entering
  `needs_input`/`paused`) and `answer` (returning it to `claimed`) have no
  `ClaimDelegationService` RPC, so a federated repository still decides those
  locally regardless of coordination mode. Whether that is a gap worth closing
  depends on whether a parked task's ownership needs to be federated at all —
  deliberately no design here; ADR-0096 itself scoped the wire contract to
  claim/renew/release/recover and left the rest to a later decision if one
  turns out to be needed.

  Distinct from *where* the coordination mode is declared (was
  gaps.d/the-coordination-mode-is-declared-per-process-not-per-repository.md,
  now closed: it is a repository-scoped git config declaration, not only a
  process environment variable).
