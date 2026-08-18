- **No repository-side client calls Ackplane's claim arbitration, so resolving
  `federated` would still report an authority that is never exercised — NARROWED
  2026-08-18, still OPEN.** The prior evidence table below is stale: it said
  "claims routed elsewhere: none". That is no longer true.

  What changed since the 2026-08-14 measurement, on `origin/main`: ADR-0096
  (leased delegation) was proposed, accepted, and is now real server-side code —
  `crates/ackplane-server/src/claim_service.rs` implements
  `ClaimDelegationService.DelegateClaim` over the exact CAS
  `lodestar-core/src/store/coordination/claim.rs` already uses, wired into
  `main.rs` and served alongside `NodeSyncService`/`NodeEnrollmentService`. A
  `ReleaseClaim` RPC (slice 2, `task:32d76e33a3bd`) is in flight as of this
  writing. So the server-side half of "no claim is arbitrated through Ackplane"
  is fixed.

  What is still true, and is the actual residual gap: no crate on the
  repository side calls any of it. `crates/ackplane-client` does not exist
  (confirmed: no such path, no such dependency in any `Cargo.toml`).
  `task:727ae37b4f5a` ("Give the repository an Ackplane client so federated
  mode can resolve") remains `blocked`, unowned, no branch. `mindleak-mcp` and
  `lodestar-mcp` still hold only their local Lodestar/MindLeak stores — nothing
  in either crate calls `ClaimDelegationService`. So a `federated` resolution
  would still mean only "a client exists and can reach the socket," not "this
  repository's claims are actually arbitrated by Ackplane" — the false
  authority ADR-0082 decision 3 refuses, and the second arbiter for one
  repository's claims ADR-0045 exists to prevent. The trap named below is
  unchanged; only which piece is missing has shrunk.

  **Why this is still a trap.** An implementer who wires *the client can call
  DelegateClaim, therefore federated resolves* still ships reachability
  presented as arbitration, unless the client actually treats the granted
  lease as authoritative over its own local claim state (ADR-0096 decision 2's
  delegation shape) rather than merely mirroring or logging the call.

  Deliberately no design here beyond what ADR-0096 already settled. Building
  `ackplane-client` against `task:727ae37b4f5a`'s existing acceptance criteria
  is the next concrete step; this fragment stops at naming that it is still
  the one true blocker.

  Distinct from
  [`the-coordination-mode-is-declared-per-process-not-per-repository.md`](the-coordination-mode-is-declared-per-process-not-per-repository.md),
  which covers *where* the mode is declared and defers its own impact to "the
  moment an Ackplane client exists". This fragment is about what must be true
  before that client can mean anything.
