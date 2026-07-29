- **`advise` no longer renews a lease, so ADR-0029 and ADR-0052 stop
  contradicting each other.** Renewal-on-activity (ADR-0052) lets any call that
  names a task prove its owner is still working, which is what stops a claim
  expiring during a long build. Its own consequences section flagged one member
  of that list as unsettled: *"`advise` should be excluded, or ADR-0029 amended —
  this decision does not get to quietly redefine another one."* The
  implementation included it and ADR-0029 was not amended, so `advise` — which
  ADR-0029 documents as evidence-free and state-free, recording no verdict and
  changing **no task state** — was writing `lease_expires_at`. Both ADRs still
  read as authoritative while the code could only satisfy one. `advise` is now
  excluded, answering the open question in the direction that keeps the existing
  contract intact; the cost is negligible, because an agent calling `advise`
  before it edits is about to call something task-bearing anyway. A test guards
  the array, so re-adding `advise` fails until someone amends ADR-0029 on
  purpose and with reasoning.
