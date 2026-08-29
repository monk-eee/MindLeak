### Added

- Added proposed ADR-0136: Ackplane gains a thin MCP protocol adapter
  (`ackplane-mcp`) translating MCP tool calls into its existing gRPC services,
  so an MCP-native client can reach the Industrial (Ackplane + Bridge +
  PostgreSQL) profile the way it reaches the Local (SQLite) profile today.
  Explicitly rejects forking `mindleak-core`/`lodestar-core` into new
  Postgres-backed sibling crates, which would duplicate business logic
  Ackplane's own server already owns. Refines ADR-0105's feature-parity
  mechanism with the MCP/agent-facing half; flags the authenticated-principal
  question and remaining task/recall domain-parity gaps as follow-up work.
