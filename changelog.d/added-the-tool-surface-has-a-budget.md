- **The advertised MCP tool surface is now a measured number, so growth can no
  longer pass unnoticed.**
  `scripts/measure-tool-surface.mjs` asks both servers for `tools/list` over
  MCP stdio and reports what a session pays to load them: 118 tools, 63.7 KB,
  roughly 16,316 tokens spent before the first question. It asks the servers
  rather than counting definitions in the Rust source, because the number that
  matters is what a client is actually served; the unit is the compact JSON
  that crosses the wire, and the token figure is bytes/4 and says so, since
  only the count is exact. A server it cannot reach fails the run instead of
  being left out — half a surface reported as the whole one reads as an
  improvement and is a missing build. Measuring cost is not judging worth, so
  the number is meant to be held by a ratchet reporting at review: whether a
  tool earns its place in the context window is a decision for a human, and
  what was missing was never the judgment but the prompt to make it. That
  ratchet is not yet registered — no active clause authorises one and a new
  clause cannot currently be given an enforcement contract (see Known gaps) —
  so for now the surface is measured and published rather than enforced. The
  ratchet is tracked separately as task:8000f45e0dfd. ADR-0059 recorded 89
  `lodestar-mcp` tools; the first run recorded 90, and the reconciled run
  recorded 91.
