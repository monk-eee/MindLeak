### Added

- Added the `ackplane-node` local IPC endpoint (ADR-0100 slice 2): a
  repository-scoped Windows named pipe or Unix-domain socket accepting only
  the closed `NodeSigner` operations (identity, sign, provision successor,
  retire, destroy) over a length-prefixed JSON protocol, never a TCP
  listener and never a reusable bearer token. A request declaring a
  repository id other than the endpoint's own is refused rather than
  serviced. On Unix the socket file is restricted to owner-only (0600)
  permissions. Enrolment/restart recovery and key rotation wiring are
  separate, narrow follow-on slices — this endpoint is not yet linked into
  `lodestar-mcp`/`mindleak-mcp`.
