### Added

- `node scripts/status.mjs` (`make status`) prints a human-runnable summary
  of live Lodestar/MindLeak state -- board health, doctor findings, live
  claims and their lease state, and MindLeak graph/telemetry health --
  reading each plane's compiled server binary directly. No agent session,
  MCP client library, or LLM call is required; `--json` prints the
  underlying data for scripting. See
  `gaps.d/no-human-runnable-status-command-outside-an-agent-session.md`.

### Fixed

- `scripts/claim-gate.mjs`'s `callTools` (shared by several scripts that
  drive the MCP servers directly) read only a tool result's Markdown
  `content[0].text` block, silently returning prose instead of data for any
  tool migrated to the dual Markdown-plus-`structuredContent` format (for
  example `lodestar_stats`, `graph_stats`, `telemetry_snapshot`). It now
  prefers `structuredContent`, matching the extension's own
  `parseToolResult`. It also accepts an optional `maxBuffer` override, since
  `task_query view=board` on a mature board can exceed `execFileSync`'s
  1 MiB default.
