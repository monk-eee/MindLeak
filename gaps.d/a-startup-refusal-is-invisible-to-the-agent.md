- **A misdeclared coordination mode stops both planes with a message no agent
  ever sees.** `resolve_coordination_mode` is the first call in `main()` in both
  [`crates/lodestar-mcp/src/main.rs`](../crates/lodestar-mcp/src/main.rs) and
  [`crates/mindleak-mcp/src/main.rs`](../crates/mindleak-mcp/src/main.rs), and
  its error leaves `main` as an `anyhow::Result`. So a
  `MINDLEAK_COORDINATION_MODE` that is misspelt (`cloud`), or set to `federated`
  before a client exists, exits the process before any store is opened, and the
  MCP client reports only a server that failed to start. This repository has
  already established that this is not enough: the stale-build notice a few
  lines below that call carries the comment "stderr alone was not enough, for
  the same reason `initialize` was not: an MCP client does not show it to the
  agent" — added after a whole session ran a two-day-old binary, read its
  verdicts as authoritative, and diagnosed two absent tools as tool defects.
  The coordination refusal has the same shape and is strictly harder to
  surface, because the remedy used there, handing the notice to `open_session`,
  is unavailable to a server that never reaches `open_session`. Impact is a
  fleet-wide stop whose diagnosis exists only in the MCP output pane, and both
  planes fail identically from one variable, so the operator sees "everything is
  broken" rather than "one declaration is wrong". Left for later, and the fix is
  explicitly NOT to fall back to local — that is the second arbiter ADR-0082
  exists to prevent. The gap is discoverability, and no clean in-band channel
  currently exists for a process that must refuse before it can serve.
