- **A dead defensive guard remains in `record_conformance_and_transition`.** —
  It errors when a predecessor has more than one successor, but
  `task_handoffs.predecessor_id` is the PRIMARY KEY, so the count is always at
  most one and the branch can never fire. — No functional impact; kept as
  documented defense-in-depth rather than removed, since the PK is the real
  guard. — Noted during the Jul 2026 audit.
