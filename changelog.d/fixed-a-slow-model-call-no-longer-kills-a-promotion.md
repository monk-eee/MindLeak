- **A slow local model no longer times out design promotion (and other
  model-backed tool calls).** The extension's MCP request timeout was a flat
  30000ms — exactly equal to the server-side model budget
  (`MINDLEAK_HTTP_TIMEOUT_MS`, default 30000). A model-backed tool call, such as
  design-promotion planning or goal decomposition, can consume that whole budget
  before it either succeeds or falls back to its deterministic plan. Because the
  two budgets were equal, the client abandoned the request at the same instant
  the server was still waiting on the model, so the clean fallback never reached
  the user: a cold `glm4:9b` load surfaced only
  `MCP request "tools/call" timed out after 30000ms` and the promotion failed.

  The request timeout is now a setting, `mindleak.requestTimeoutMs`, defaulting
  to 300000ms so the client budget outlasts the bounded default model sequence,
  including ADR-0079's one cold-start read-timeout retry. It is wired into both
  MCP planes (memory and intent) and takes effect on window reload. A
  settings-surface test calculates the full default sequence and requires the
  manifest and activation defaults to agree above it, so the budgets cannot
  silently converge again.
