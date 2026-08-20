### Added

- The Ackplane v1 synchronization stream now defines the closed, additive
  `AgentDirective` and `DirectiveReceipt` wire contracts from ADR-0107. Ackplane
  can carry typed notify, prompt, assign, steer, pause, resume, drain, and
  terminate directives to an enrolled agent supervisor, and the node can return
  accepted, refused, applied, failed, or expired receipts with immutable routing,
  sequence, expiry, idempotency, payload-digest, provenance, checkpoint, and
  evidence metadata. This release adds only the compatible Protobuf contract;
  delivery, persistence, authorization, and supervisor behavior remain separate
  implementation slices.
