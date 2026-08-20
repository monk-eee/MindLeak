### Added

- Bridge repository detail now shows the health of every enrolled signing key:
  node, fingerprint, and status (resolved, expired, revoked, retired, or not
  yet active), judged as of now rather than as of some past envelope's
  acceptance. The new tenant-scoped
  `GET /api/v1/repositories/:repository_id/signing-keys` read returns `404`
  for an unenrolled or cross-tenant repository, and reuses the same
  `signing_keys::judge` rule an accepted envelope's own verification applies
  rather than a second judgment invented for the health view.
