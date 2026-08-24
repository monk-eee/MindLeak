- **`ackplane-client` cannot yet *decide* whether a repository is enrolled,
  only ask when it already has a candidate identity to ask about.**
  ADR-0122 added `NodeEnrollmentService.CheckEnrollmentStatus`, and
  `EnrollmentClient::check_enrollment_status` now calls it for real
  (`tests/check_enrollment_status.rs`: a real activated node's own check
  verifies as `Active`; a never-enrolled candidate reports `verified: false`).
  So the wire-contract gap this fragment originally named — no RPC existed to
  answer an enrolment question — is closed. What remains open: nothing in
  `ackplane-client` or `ackplane-core` sources the candidate `node_id` /
  `key_fingerprint` / signing key a `federated` repository would pass to that
  call on its own behalf, so `compiled_federation_readiness` still only
  distinguishes `Ready` from `ArbiterUnreachable` and can never produce
  `FederationReadiness::NotEnrolled` — that identity-sourcing question is its
  own reviewed decision (how does a repository durably hold its own candidate
  identity and key between processes?), not a side effect of adding the RPC
  call, and is not fixed here.
- **Narrowed further**: `ackplane_client::identity` now sources and persists
  a repository's candidate `node_id` / `key_fingerprint` / signing key --
  `register-me request`/`activate` write it, `identity::load_candidate_identity`
  reads it back, and `identity::signed_status_request` builds a signed
  `CheckEnrollmentStatus` request from it. This is usable today by
  `register-me`'s own CLI bootstrapping, which already held a raw key file on
  disk before this change; `register-me`'s `--key-path` now defaults to
  `identity::DEFAULT_KEY_PATH` so `request` and `activate` agree on the same
  path without a human passing it between the two calls.
- **What remains open, deliberately**: `ackplane_core::compiled_federation_readiness`
  -- the function every `federated` local plane calls once at startup --
  still only distinguishes `Ready` from `ArbiterUnreachable`, never
  `NotEnrolled`. Wiring `identity::load_candidate_identity` into it so it
  could also answer `NotEnrolled` would mean `lodestar-mcp`/`mindleak-mcp`
  loading and signing with this repository's raw private key directly at
  startup -- exactly what ADR-0100 decision 3 forbids ("neither plane calls
  an OS key API ... or receives a raw signature primitive"). That answer has
  to come from the `ackplane-node` companion's non-exporting signer
  (ADR-0100), which does not exist yet as a crate or binary. Do not close
  this by wiring the raw-key path into a local plane's own startup check;
  build the companion first.
