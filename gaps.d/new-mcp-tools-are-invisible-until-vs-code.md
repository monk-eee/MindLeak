- **New MCP tools are invisible until VS Code reloads, and the binaries cannot be
  rebuilt while it runs — OPEN.** — `cargo build --release` fails with `Access is
  denied (os error 5)` on `lodestar-mcp.exe` / `mindleak-mcp.exe` because the
  running servers hold the files open. So a session that adds a tool cannot
  exercise it, and there is no in-band signal that the advertised tool list is
  stale — the tool simply does not exist. — Low impact, high friction: purely an
  inner-loop cost, but it silently blocks end-to-end verification of anything
  added to the MCP surface within the same session. — Left for later; workaround
  is to reload the window (or restart the servers) before verifying new tools.
