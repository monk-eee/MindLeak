### Added

- Ackplane now exposes typed, serializable enrolled-supervisor protocol models
  for a stable tenant/repository/node identity, capability declaration,
  durability truthfulness, worker sessions, and lifecycle receipts. The model
  rejects empty identity/version fields, empty or duplicate control
  capabilities, force-termination declaration mismatches, and an ephemeral
  outbox claiming recovery after process loss before later ADR-0116 runtime,
  transport, and durable-store work consumes it.
