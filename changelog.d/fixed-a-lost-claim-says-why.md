- **A lost claim says why, and what to do about it.** `claim_task` returned a
  bare `won: false`. The reasons a compare-and-swap can miss call for opposite
  responses — wait for a live lease, pick different work because the task is
  finished, unblock a predecessor, or rebuild a stale server binary — so one
  boolean covering all of them is not terse, it is unusable. It also meant
  `scripts/claim-gate.mjs` had to exist: a whole diagnostic written to
  reconstruct, after the fact, what the plane knew at the moment it refused.
  A refusal now names the holder and the remaining lease, points at
  `recover_claim` when the lease has lapsed, distinguishes finished work and
  missing work from contended work, and — the expensive one — recognises a
  claim held under a pre-session identity as a stale binary (ADR-0054) rather
  than a live claim, saying so and warning that re-claiming will not help.
  `owner`, `status`, `lease_expires_at` and `blocked_by` come back alongside,
  so a caller can branch on the outcome instead of parsing prose.
