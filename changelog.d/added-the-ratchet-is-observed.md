- **The module-length ratchet is now observed, not merely registered.**
  `control:rust-module-length` had a reviewed baseline and a committed measurer
  and nothing ever told it anything — the same shape as the six script suites
  and the merged-branch audit found earlier the same day: a mechanism that
  exists, works, and runs nowhere. `scripts/observe-module-length.mjs` measures
  the governed modules and reports the count through `observe_ratchet`, and it
  runs on every publication, because publication is when the work becomes
  visible to the fleet and therefore the honest moment to measure what the fleet
  now has to live with. It reports locally rather than in CI on purpose: the
  Intent Plane is a per-developer store, so an observation recorded on a
  throwaway runner is recorded nowhere. It never blocks a push — the clause
  resolves at `review` and the control's power is `observed`, so failing a push
  on a regression would enforce harder than the rule it serves (ADR-0034); a
  rising count is a question for a human, and cohesion still outranks size. What
  it does refuse is running blind: an unattributed session or an unreachable
  Intent Plane fails loudly, because a reporter that quietly says nothing is
  indistinguishable from one reporting a pass.
