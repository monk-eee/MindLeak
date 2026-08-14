- Ackplane now records an activated node's signing key, so an `EventEnvelope`'s
  `signing_key_id` resolves to the key that signed it. Until this landed the
  ledger persisted a key id that referred to nothing, and no signature in the
  system could be verified however finished the surrounding shape looked.
- A key's authority is judged as of the moment an envelope was accepted, not as
  of the lookup. An envelope accepted while its key was valid stays resolvable
  after that key is later rotated away from or revoked, and a lookup reports
  which of unknown, wrong-node, not-yet-active, expired, retired or revoked
  applies rather than collapsing them into a single failure.
