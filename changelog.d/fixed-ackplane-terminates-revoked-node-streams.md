- Ackplane's `NodeSyncService` now terminates a node's live stream once its
  signing key is revoked (ADR-0085 decision 8), instead of leaving the
  connection open to keep sending records that would all be refused the same
  way. A revoked key's rejection now carries its own `node_revoked` reason,
  distinct from the ordinary not-yet-active/expired/retired case, so an
  operator can tell an administrative revocation from routine key lifecycle.
