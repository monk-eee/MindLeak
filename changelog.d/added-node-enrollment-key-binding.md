- Persistent node keys now begin as typed enrollment candidates, persist and
  replay approved challenges, and bind the authority's returned key ID and
  receipt without replacing or exporting the key. Recovery completes interrupted
  metadata writes and refuses missing or mismatched credentials. Node and server
  use one canonical fingerprint and activation encoder, fixing the provider's
  incompatible short fingerprint. Real-service tests prove activation replay and
  two authenticated reconnects with one key and receipt. CLI and supervisor
  integration remain open; this does not import existing seed files or claim
  hardware-backed key custody.
