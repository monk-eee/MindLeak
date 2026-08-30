- **Observed:** `ackplane_client::node_identity::resolve_node_identity` (added
  alongside ADR-0137 clause 1's `ackplane-mcp` node-trust handshake) is now a
  third, independent implementation of the same `MINDLEAK_ACKPLANE_TENANT_ID`/
  `_REPOSITORY_ID`/`_NODE_ID`/`_SIGNING_KEY_ID`/`_NODE_SIGNING_KEY_SEED`
  environment-variable resolution and `SeedSigner`/`CredentialFacilitySigner`
  selection logic that `ackplane-supervisor`'s
  [`config.rs`](/crates/ackplane-supervisor/src/config.rs) and
  `lodestar-mcp`'s
  [`federation.rs`](/crates/lodestar-mcp/src/federation.rs) each already
  carry, unaware of each other.
- **Where:** [`crates/ackplane-client/src/node_identity.rs`](/crates/ackplane-client/src/node_identity.rs)
  is the new shared version; the other two were left untouched to keep the
  ADR-0137 clause 1 slice narrowly scoped to `ackplane-mcp`.
- **Impact:** a fourth caller reaching for this pattern has one correct,
  shared place to extend (this module) rather than a fourth private copy,
  but the existing two duplicates still drift independently of it and of each
  other -- a bug fixed in one (e.g. the credential-facility account-naming
  scheme) will not automatically reach the other two.
- **Fixed this run:** no. Migrating `ackplane-supervisor::config` and
  `lodestar-mcp::federation` onto `ackplane_client::node_identity` is a
  separate, mechanical refactor with its own review (each has its own
  `SupervisorConfig`/`FederationIdentity` struct shape callers already
  depend on), out of scope for a slice whose job was `ackplane-mcp`'s new
  behaviour.
