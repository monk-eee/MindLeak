- **Ackplane now maps a node's wire envelopes onto ledger appends and answers
  with typed receipts.** `ackplane-server` gained the decision layer between
  ADR-0083's `mindleak.ackplane.v1` contract and the ledger: an accepted record
  returns the position it was written at, an identical retry returns the
  *original* position as a `duplicate`, and the same producer sequence sent with
  a different envelope digest is refused as a non-retryable sequence conflict.
  Every refusal carries a stable reason code, the offending record's identity,
  and whether retrying could ever succeed, so a client never has to branch on
  human-readable text.
- **A node can no longer declare a payload digest unrelated to its own
  payload.** The digest decides whether a repeat is a duplicate or a conflict,
  so an unverified one let a sender choose that outcome for itself; the ledger
  could not catch it, because it only ever compares the declared digest to the
  one already stored. It is now checked against the payload at the wire
  boundary and refused as malformed.
- A ledger failure reports only that the ledger was unavailable and the retry is
  worth making. The underlying database error is logged rather than returned,
  because it can name hosts, roles and schema that a remote node needs none of
  ([ADR-0083](../docs/adr/0083-grpc-is-the-ackplane-node-protocol.md),
  [ADR-0086](../docs/adr/0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)).
