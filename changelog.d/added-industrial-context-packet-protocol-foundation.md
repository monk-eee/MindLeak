### Added

- Ackplane now exposes a typed, serializable `ContextPacket` protocol model for
  the Industrial guidance loop. The model binds one packet to its tenant,
  repository, task, goal, agent session, compiler/source versions, token
  budget, selected provenance-bearing items, explicit exclusions, and
  packet-use receipts. It rejects malformed identity/scope, expired packets,
  budget overruns, duplicate selections, and selected/excluded overlap before
  future ContextService or durable-store work consumes it under ADR-0114.
