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
