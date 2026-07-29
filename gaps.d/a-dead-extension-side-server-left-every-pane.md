- **A dead extension-side server left every pane blank and the health line
  lying — FIXED.** — Observed Jul 2026: the MindLeak views were all
  empty while the agent-facing `mcp_*` tools worked normally. The extension
  spawns its **own** `mindleak-mcp` / `lodestar-mcp` children (`McpClient` in
  [`editors/vscode/src/mcpClient.ts`](editors/vscode/src/mcpClient.ts), resolved
  by `resolveBinaryPath` to the *bundled* `bin/`, not `target/release`), and the
  previous session's `taskkill` — the documented step before rebuilding the
  release binaries — killed them. Nothing restarted them, so the panes stayed
  dead for hours until the extension host happened to restart. The health line
  compounded it: `activate()` recorded `memory connected` once and never revised
  it, so the one surface that should have said something was confidently wrong.
  — Medium impact: no data loss, but the product looks broken and the cause is
  invisible unless you think to open the output channel. — Fixed Jul 2026: the
  client relaunches the server itself (three consecutive attempts, then it says
  a reload is needed), no longer logs from the exit handler during disposal —
  which was raising `Channel has been closed` in the extension host log — and
  publishes `connected` / `reconnecting` / `disconnected` to a state listener
  that the extension maps onto the plane's health line. The four independent
  health strings collapsed into the `RuntimeHealth` record they already modelled,
  behind one change-guarded `setHealth`. Note the fix is TypeScript, so an
  **installed** extension keeps the old behaviour until it is rebuilt and
  reloaded.
