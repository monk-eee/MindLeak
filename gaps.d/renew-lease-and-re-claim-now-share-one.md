- **`renew_lease` and re-claim now share one evidence-window rule.** — Renewal is
  a heartbeat for a still-live lease and preserves `claim_started_at`; it refuses
  after expiry. A lapsed owner must win `claim_task` again, which resets
  `claim_started_at` and opens a fresh conformance evidence window. The guarded
  single-statement CAS and both paths are regression-tested. — Resolved Jul 2026.
