- **Added:** `scripts/mcp-direct.mjs` drives a batch of Lodestar/MindLeak tool
  calls directly against the built release binaries over the same
  newline-delimited JSON-RPC stdio `scripts/canonical-push.mjs` already
  speaks on every publish, independent of any editor's persistent MCP
  connection. That connection can break for one session and not recover on
  its own, even across a window reload (see
  `gaps.d/mcp-server-processes-accumulate-per-editor-window.md`), while the
  underlying binaries and their SQLite-backed state are otherwise
  unaffected. `node scripts/mcp-direct.mjs <lodestar|mindleak> calls.json`
  takes a JSON array of `{"name", "arguments"}` calls and runs them as one
  batch, since each server process is stateless across invocations and
  `open_session` only has anything to do with what follows it in the same
  process.
