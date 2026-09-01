- **Observed:** `ackplane_client::node_identity::resolve_node_identity` (added
  alongside ADR-0137 clause 1's `ackplane-mcp` node-trust handshake) was a
  third, independent implementation of the same `MINDLEAK_ACKPLANE_TENANT_ID`/
  `_REPOSITORY_ID`/`_NODE_ID`/`_SIGNING_KEY_ID`/`_NODE_SIGNING_KEY_SEED`
  environment-variable resolution and `SeedSigner`/`CredentialFacilitySigner`
  selection logic that `ackplane-supervisor`'s
  [`config.rs`](/crates/ackplane-supervisor/src/config.rs) and
  `lodestar-mcp`'s
  [`federation.rs`](/crates/lodestar-mcp/src/federation.rs) each already
  carried, unaware of each other.
- **Where:** [`crates/ackplane-client/src/node_identity.rs`](/crates/ackplane-client/src/node_identity.rs)
  is the shared version.
- **Impact:** a fourth caller reaching for this pattern has one correct,
  shared place to extend (this module) rather than a fourth private copy.
- **`lodestar-mcp::federation` FIXED:** `resolve_identity` now delegates its
  shared-field resolution (tenant/repository/node/signing-key ids and
  signer-source selection) to `node_identity::resolve_node_identity`,
  adding only the one field specific to a federation connection
  (`endpoint`) on top; `IDENTITY_ENV_VARS` now references the shared
  module's own env-var name constants instead of re-declaring them. The
  public `FederationIdentity`/`SignerSource` shapes and `resolve_identity`'s
  signature are unchanged, so `main.rs`'s call site needed no edit.
- **`ackplane-supervisor::config` STILL OPEN** (not fixed by this commit):
  confirmed via `grep` that `crates/ackplane-supervisor/src/config.rs` does
  not yet reference `node_identity` on this branch's base -- the migration
  for this half is on branch `fix/node-identity-single-resolution` (PR #860),
  which is unmerged and, as of this commit, still being reconciled against a
  since-diverged `main` under a live Lodestar claim. Re-verify and close (or
  narrow further) once that PR lands.
