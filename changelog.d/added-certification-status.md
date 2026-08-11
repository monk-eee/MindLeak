- **A subject now holds a qualified certification status.** `certification_status`
  projects the deterministic conformance record a task already closed on into the
  status line ADR-0090 §2 describes: subject, commit, the policy version judged
  against, the evidence bundle, the date, and which clauses were evaluated and
  which were not. No new judgement runs, and no component is named for the claim
  it emits. The states are distinct and none renders as certified — `certified`,
  `not_certified` with its reason, `waived` with its expiry and remediation,
  `needs_human`, `uncertifiable` where no constitution has been adopted, and
  `stale` where the subject moved past its evidence. An `aligned` verdict over a
  bundle covering no node is reported as `not_certified`, because agreement about
  nothing is not proof. Staleness is judged against the commit the caller
  declares, since the server never reads Git. Nothing here asserts compliance
  with an external framework: a status certifies conformance to the clauses it
  names and nothing more (ADR-0090).
