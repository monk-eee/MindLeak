- **`ackplane-client` cannot tell "the arbiter is unreachable" apart from "the
  arbiter is reachable but does not recognise this repository."** `probe_reachable`
  (and therefore `compiled_federation_readiness`) only opens a transport
  connection; today's `ackplane-protocol` wire contract has no RPC that
  answers an enrolment question (`NodeEnrollmentService` submits and activates
  requests, but nothing asks "is repository X already enrolled?"). A
  repository declared `federated` whose Ackplane deployment is up but has
  never enrolled it currently resolves the same way as one whose deployment is
  down: `FederationReadiness::ArbiterUnreachable`. The remedy text ("check the
  deployment, or declare local") is misleading for the unenrolled case — the
  deployment is fine, the repository just was never registered with it. Not
  fixed here: doing so needs a new query RPC on `NodeEnrollmentService` or
  `ClaimDelegationService`, which is its own reviewed wire-contract decision,
  not a side effect of `task:727ae37b4f5a`'s client-existence scope.
