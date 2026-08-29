### Added

- `ackplane-mcp` gains `open_session` (ADR-0137 clause 2): identity is the
  session, not the process, exactly as `mindleak-mcp`/`lodestar-mcp` already
  work. It shares the `mindleak-session` crate, so it mints the same
  `session:v1:<hex>` identity form, accepts the same optional declared
  working-context fields (`branch`, `head_sha`, `base`, `dirty`, `behind`),
  and consults no Ackplane endpoint to open a session -- node-level
  connection trust (the existing loopback pilot) authenticates the process;
  `open_session` layers an independently-declared agent identity on top.
  `open_session` is refused exactly like every other tool when the front
  door cannot reach its arbiter: there is no weaker, arbiter-free path
  through that refusal. Set `ACKPLANE_MCP_AGENT` to label this process's
  sessions in reports, mirroring `MINDLEAK_AGENT`/`LODESTAR_AGENT`.
