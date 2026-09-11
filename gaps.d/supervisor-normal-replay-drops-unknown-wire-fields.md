- **Ordinary outbox replay does not preserve unknown protobuf fields.**
  `QueuedFrame::decode` in `crates/ackplane-supervisor/src/outbox.rs` decodes a
  stored `NodeFrame`; `daemon::resend_pending` sends the decoded value, which
  re-encodes without unknown fields. A future schema or altered queue can
  therefore produce different replay bytes. Fixed this run for operator
  recovery: `QueuedFrame::decode_exact` refuses non-identical encodings before
  confirmation, tested by
  `recovery_refuses_changed_wire_bytes_even_when_the_decoded_receipt_is_equal`.
  General delivery remains unchanged and needs a separate exact-byte or
  explicit supported-encoding contract before relying on forward-schema replay.
