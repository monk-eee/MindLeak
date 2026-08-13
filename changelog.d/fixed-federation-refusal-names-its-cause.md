- **A `federated` declaration that cannot be honoured now says which of three
  things is wrong.** `MINDLEAK_COORDINATION_MODE=federated` used to fail with a
  single `FederationUnavailable` whose message asserted that the build carried
  no Ackplane client. That was true while no client existed, and it was about to
  become wrong in the most expensive way: once a client ships, an operator whose
  arbiter is simply down would have been told to rebuild a binary that was
  already correct, and the actual remedy would have appeared nowhere. The
  refusal now distinguishes a build with no client compiled in, an arbiter that
  did not answer, and a repository the arbiter does not recognise, and only the
  first is fixed by changing the binary — the other two say so explicitly.
  Whether federation is usable is now passed in rather than assumed, so the
  accepting arm can be exercised for the first time; nothing about the local
  path changes, and an unusable arbiter is still refused rather than quietly
  downgraded to local, which is what would give one repository two arbiters for
  the same claims (ADR-0082 decision 3, ADR-0045).
