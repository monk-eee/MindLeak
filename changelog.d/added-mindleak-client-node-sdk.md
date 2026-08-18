### Added

- Packaged Node.js client (`clients/node/mindleak-client`) implementing
  ADR-0103: a typed wrapper over `mindleak-mcp`/`lodestar-mcp`'s stdio
  JSON-RPC contract, with service groups for knowledge, tasks, evidence, and
  graph reads over a generic `callTool` escape hatch.
