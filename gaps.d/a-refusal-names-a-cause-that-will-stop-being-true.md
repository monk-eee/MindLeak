- **`FederationUnavailable` names a cause that will stop being true, in the one
  variant that outlives it.** `CoordinationMode::ensure_supported` in
  [`crates/ackplane-core/src/lib.rs`](../crates/ackplane-core/src/lib.rs)
  refuses `federated` unconditionally, and the error reads "this build carries
  no Ackplane client and cannot reach a federated arbiter". Both halves are
  exactly true today, because no client exists: the workspace holds
  `ackplane-core` (the mode enum and this refusal, 147 lines),
  `ackplane-protocol` (the pinned wire contract), and `ackplane-server`
  (~2,000 lines of ledger, projection and sync), and nothing on the repository
  side that can speak to any of it. The moment a client does exist, those two
  halves become three different failures with three different remedies — a
  build compiled without the client, an arbiter that is down, a repository that
  was never enrolled — and one variant carrying one message will be raised for
  all of them. The comment above the function anticipates the change as "the
  day an Ackplane client exists, only this answer changes", which understates
  it: the error type has to split in the same commit, or the first person to
  meet an unreachable arbiter is told to rebuild a binary that is already
  correct. Impact is entirely future and currently zero — today the message is
  accurate and the refusal itself is right, because ADR-0082 decision 3 forbids
  falling back to local on reachability. Left for later; close this when the
  client lands, by splitting the variant rather than by editing the prose.
