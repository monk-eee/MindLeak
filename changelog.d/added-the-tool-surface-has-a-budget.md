- **The advertised MCP tool surface is now a measured number, so it can no
  longer grow without anyone deciding to grow it.**
  `scripts/measure-tool-surface.mjs` asks both servers for `tools/list` over
  MCP stdio and reports what a session pays to load them: 117 tools, 61.6 KB,
  roughly 15,800 tokens spent before the first question. It asks the servers
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
  so for now the surface is measured and published rather than enforced.
  ADR-0059 recorded 89 `lodestar-mcp` tools; the first run of this script, a
  day later, recorded 90.
