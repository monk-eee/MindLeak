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
- **Provider-backed CLI update:** `ackplane_client::identity` still reads the
  legacy candidate/key files and builds signed status requests from them, but
  `register-me` no longer writes those files. The CLI now uses
  `ackplane-node::CredentialCandidate` and `CredentialProvider`, requires an
  explicit provider/state directory, and rejects raw key options. The MCP
  status loader has not adopted this lifecycle, so successful provider-backed
  enrollment can still produce an "unable to ask" local status response. This
  remaining handoff is open, not a reason to reintroduce seed-file copying.
- **What remains open, deliberately**: `ackplane_core::compiled_federation_readiness`
  -- the function every `federated` local plane calls once at startup --
  still only distinguishes `Ready` from `ArbiterUnreachable`, never
  `NotEnrolled`. Wiring `identity::load_candidate_identity` into it so it
  could also answer `NotEnrolled` would mean `lodestar-mcp`/`mindleak-mcp`
  loading and signing with this repository's raw private key directly at
  startup -- exactly what ADR-0100 decision 3 forbids ("neither plane calls
  an OS key API ... or receives a raw signature primitive"). That answer has
  to come from the `ackplane-node` companion's non-exporting signer
  (ADR-0100), whose tested provider library exists but whose long-lived
  companion and closed status operation are not yet wired. Do not close
  this by wiring the raw-key path into a local plane's own startup check;
  build the companion first.
