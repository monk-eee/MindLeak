### Added

- Added proposed ADR-0137, resolving ADR-0136's open authenticated-principal
  question: `ackplane-mcp` authenticates by borrowing an already-enrolled
  repository node's Ed25519 key (the existing NodeSync challenge-response,
  ADR-0085/0098), the same mechanism `ackplane-supervisor` already uses.
  Individual MCP client identity layers on top via the existing
  `open_session(session_id)`/`agent_session_id` mechanism (ADR-0030/0054),
  unchanged from how the local planes already work. Explicitly defers a
  stronger multi-user principal (revocable API key or OIDC) as sequenced
  future work rather than blocking a first pilot on it.
