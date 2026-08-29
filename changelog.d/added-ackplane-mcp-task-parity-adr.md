### Added

- Added proposed ADR-0139, resolving ADR-0136's open task/claim domain-parity
  question: `ackplane-mcp`'s first task-related tool surface composes
  Ackplane's already-federated claim arbitration (claim/renew/release/recover,
  ADR-0096) with its read-only Work projection (ADR-0120), and explicitly
  refuses every lifecycle-mutation operation (create, pause/resume, answer
  wait, complete/abandon) that ADR-0120 decision 8 itself already defers
  server-side. `ackplane-mcp` does not fake these operations client-side, and
  does not present its narrower task surface under the same names as
  `lodestar-mcp` without disclosing the gap.
