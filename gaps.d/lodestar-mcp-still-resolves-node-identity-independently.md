- **Observed:** `lodestar-mcp`'s
  [`federation.rs`](/crates/lodestar-mcp/src/federation.rs) still carries its
  own copy of the `MINDLEAK_ACKPLANE_TENANT_ID`/`_REPOSITORY_ID`/`_NODE_ID`/
  `_SIGNING_KEY_ID`/`_NODE_SIGNING_KEY_SEED` resolution, its own
  `SignerSource` enum, its own `CREDENTIAL_FACILITY_SERVICE` constant, its own
  `credential_facility_account` formatter and its own signer construction,
  rather than using
  [`ackplane_client::node_identity`](/crates/ackplane-client/src/node_identity.rs).
  This was previously a *triplication*; `ackplane-supervisor` and
  `ackplane-mcp` have since been migrated onto the shared module, so one
  independent copy remains.
- **Where:** [`crates/lodestar-mcp/src/federation.rs`](/crates/lodestar-mcp/src/federation.rs)
  -- `FederationIdentity`, `SignerSource`, `resolve_identity`,
  `credential_facility_account`, `IDENTITY_ENV_VARS`, and
  `AckplaneClaimAuthority::signer`.
- **Impact:** a change to the credential-facility account scheme, the service
  name, or the seed encoding now reaches `ackplane-supervisor` and
  `ackplane-mcp` together but still has to be applied to `lodestar-mcp`
  separately. Two implementations can disagree about which key a given
  enrolled node signs with, and nothing fails loudly when they do -- the
  federated claim path would simply authenticate as something the other
  processes do not.
- **Why it was not folded into the same change:** `federation.rs` is governed
  by `goal:durable-intent-plane-for-multi-agent-coordinatio`, while the shared
  module and the other two callers are governed by
  `goal:ackplane-federation-service`. Doing all three at once produces a task
  spanning two goals, which caps its conformance verdict at `needs_human`
  rather than earning a clean one. Splitting it keeps each change reviewable
  under the goal that actually governs it.
- **Extra care this one needs:** `FederationIdentity` is `NodeIdentity` plus
  an `endpoint`, and its `SignerSource::Seed` holds `[u8; 32]` where the
  shared module holds `Box<[u8; 32]>`; `IDENTITY_ENV_VARS` also includes
  `ACKPLANE_ENDPOINT_ENV`, which is not part of the node identity.
  `ackplane-client` is an *optional* dependency there, behind the
  `federation-client` feature, so the migration must stay inside that feature
  gate.
- **Fixed this run:** no -- this is the remaining third of the original
  triplication, left OPEN deliberately and scoped above.
