### Changed

- Accepted ADR-0137 (`ackplane-mcp` authenticates by borrowing an enrolled
  node's key), ADR-0139 (`ackplane-mcp`'s task surface scopes to Ackplane's
  existing claim and read authority, not full Lodestar parity), and ADR-0140
  (a `pgvector` recall store scoped to `projected_nodes`, not the curated
  `knowledge` domain) — repository owner, authorized directly in session.
  Together with the already-accepted ADR-0136, all four decisions the
  `ackplane-mcp` MCP front door needed are now settled rather than merely
  proposed.
