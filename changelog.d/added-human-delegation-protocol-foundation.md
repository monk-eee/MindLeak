### Added

- Ackplane now exposes typed, serializable human-delegation protocol models for
  a bounded tenant/repository/project/task scope, a named human issuer and
  agent-session recipient, routine-only delegated actions, policy version,
  token/action limits, effective/expiry window, status, and receipts. The
  foundation rejects blank identities or optional scope fields, empty or
  duplicate action sets, non-positive limits, and invalid time windows before
  later ADR-0115 durable authorization, Bridge decision, and runtime
  enforcement work consumes it.
