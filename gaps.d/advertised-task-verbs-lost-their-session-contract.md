- **Advertised task verbs lost their session contract — OBSERVED, FIXED.**
  Found by dogfooding the release binary from PR #201: `open_session` succeeded,
  then `task_transition` with `to="complete"`, exact merge evidence and its
  exact conformance check failed with `a registered session agent is required`.
  The identical chain succeeded through deprecated `complete_task`. In
  `crates/lodestar-mcp/src/tools/mod.rs`, `requires_session`, optional-session
  handling and the heartbeat table were all keyed by the pre-collapse names,
  so the advertised calls bypassed identity resolution and proof-of-life while
  their aliases still worked. This also made `task_claim` unusable through the
  advertised MCP schema and made `task_query` overlap signals lose requester
  branch context. Fixed in this run by canonicalizing renamed calls before an
  operation-aware required/optional/no-session decision and by using the same
  canonical call for heartbeat policy. Regression tests reproduce the failure
  through `bind_session`, pin conditional schema behavior, and prove legacy and
  advertised heartbeat parity.
