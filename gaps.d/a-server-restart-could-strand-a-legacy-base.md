- **A server restart could strand a legacy base-id claim until lease expiry — FIXED.** —
  This run claimed work while the configured identity was the legacy `copilot`;
  after the ADR-0030 server restart the process identity became nonce-qualified,
  so owner-guarded lifecycle operations correctly refused the old owner's live
  claim. — Medium migration impact: work is preserved, but the new process must
  wait for lease expiry. — `recover_claim` now requires expiry/grace, exact owner,
  compatible base, and a reason; it starts a fresh window and appends the prior
  owner/window/status to `task_claim_transfers`. Live claims, wrong bases, and
  qualified sibling sessions are refused.
