- **The repository-side node signing key used to authenticate federated claim
  requests is sourced from a plaintext environment variable
  (`MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED`), not an OS credential facility.
  MEASURED 2026-08-18, left OPEN.** `ackplane-client::auth::SeedSigner`
  constructs an Ed25519 signing key directly from a 32-byte seed the caller
  supplies; `lodestar-mcp`'s `federation.rs` reads that seed as a hex string
  from the environment, the same posture already accepted here for
  `MINDLEAK_LLM_API_KEY`. ADR-0085 decision 2 says the private key "is stored
  through an operating-system credential facility where available;
  otherwise... an explicitly configured workload secret provider" — no such
  integration exists anywhere in this workspace yet, for any repository-side
  caller, so this is the first piece of production code that actually needs
  to hold and use a node's private key, and it does so with the simplest
  correct-but-not-hardened mechanism available today.

  This does not weaken anything ADR-0096 or ADR-0098 already verify: the
  signature itself is still checked server-side against the enrolled public
  key exactly as any other signer's would be, and a seed sourced from the
  environment is no less real a private key than one sourced any other way —
  it is only less protected against exfiltration from the host running this
  process. Fixing it is its own task: an OS-credential-facility-backed (or
  explicitly configured workload-secret-provider-backed) implementation of
  `ackplane_client::auth::ClaimSigner`, swapped in wherever
  `AckplaneClaimAuthority` is constructed, with no change needed to the
  claim-routing logic itself — `ClaimSigner` is already the seam that
  implementation would fill.
