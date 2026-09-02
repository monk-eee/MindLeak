### Added

- Added `ackplane-node` enrolment + restart identity recovery (ADR-0100
  slice 3): on first enrolment, the non-secret half of a node's identity
  (tenant/repository/node id, provider scheme, key id, public key,
  fingerprint — never the private key) is persisted; on restart, the
  current provider's identity is compared against that record before any
  stream is opened or claim is acquired. A missing or mismatched record
  refuses as `identity_unavailable` rather than silently minting a
  replacement identity. Submitting the public key/fingerprint to Ackplane
  itself is a separate follow-on once a real Ackplane client is wired into
  this crate.
