- The Ackplane enrolment request now carries the node's public key, not only its
  fingerprint. A fingerprint is a hash of a key, so the shipped contract could
  not support ADR-0085's requirement that Ackplane verify a node's activation
  signature — the only key bytes anywhere were on key *rotation*, which
  presupposes an enrolled key to rotate from, leaving the trust chain with no
  origin. The field is additive at a fresh number and existing clients decode
  unchanged.
