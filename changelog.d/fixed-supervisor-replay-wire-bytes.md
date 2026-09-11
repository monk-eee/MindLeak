- Supervisor outbox replay now refuses stored protobuf encodings that this version
  cannot reproduce byte-for-byte, instead of silently discarding unknown fields.
  Ordinary delivery, archive inspection and recovery share the same check. The
  refusal identifies the sequence and retains the original bytes and delivery
  position; no frame in the loaded batch is sent or acknowledged.
