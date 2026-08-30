### Added

- `ackplane-mcp` authenticates its Ackplane connection using an enrolled
  node's own key (ADR-0137 clause 1), completing the same
  `Hello -> ConnectionChallenge -> ChallengeResponse -> HelloAccepted`
  handshake `ackplane-supervisor` already performs at startup, rather than
  minting a new principal type. When an operator declares that node's
  identity via the same `MINDLEAK_ACKPLANE_TENANT_ID`/`_REPOSITORY_ID`/
  `_NODE_ID`/`_SIGNING_KEY_ID`/`_NODE_SIGNING_KEY_SEED` variables
  `ackplane-supervisor` and `lodestar-mcp`'s federated claim path already
  read (`ackplane_client::node_identity`, extracted as the shared
  implementation a third caller of this pattern reuses), the handshake must
  succeed or the whole process refuses to serve, naming the declared node id
  and citing ADR-0137 clause 1, exactly like the existing endpoint refusal.
  A process with no node identity declared at all is unaffected by this
  check today -- a named, deliberate limitation of this slice, not a silent
  one. Clause 6's open question (whether Ackplane's `NodeSync` protocol
  tolerates a second connection signed by the same node key while
  `ackplane-supervisor`'s own connection is still open) is now confirmed by
  a real, gated integration test.
