- The VS Code extension now provides both MCP servers itself, and the committed
  `.vscode/mcp.json` is gone (ADR-0073). The extension contributes them through
  `mcpServerDefinitionProviders`, rooting each server at the workspace folder of
  the window that provides it, so the rooting behaviour ADR-0073 established is
  unchanged. Where a binary lives is now decided by one tested rule —
  `resolveBinaryPath` — instead of a config file carrying a second, untested copy
  of it, and a new machine no longer needs a hand-edited config to reach the
  servers.
- **The extension now requires VS Code 1.101 or newer**, up from 1.93. The MCP
  extension API shipped in 1.101 (May 2025). `engines.vscode`, `@types/vscode`,
  the pinned Extension Host smoke version and its CI job name all move together,
  because a smoke job on 1.93 testing code that needs a later API is a green
  build that proves nothing. `@types/vscode` is pinned exactly to `1.101.0`
  rather than a caret, which resolves forward and would let code compile against
  APIs the declared floor does not have.
  This is a real support cut: the graph views and the passive sensor did work on
  1.93, because the extension speaks MCP through its own client. What never
  worked there is the editor's own MCP support — 1.93 shipped in August 2024 and
  MCP was announced that November — so the old floor advertised a version on
  which MindLeak's purpose was impossible.
- Server resolution now prefers the shared install at `~/.mindleak/bin` over a
  worktree's own `target/` build. Reusing the previous order would have
  reinstated the per-worktree binary ADR-0073 rejected on measurement (56
  worktrees, 184 GB of build output, only 15 holding a server binary). A side
  effect: the extension's own client and the servers offered to chat agents can
  no longer resolve to different builds, which they previously could.
- Action required in this repository: install the extension build that contains
  the provider (`npm --prefix editors/vscode run package:vsix`, then
  `code --install-extension`). There is no committed config to fall back on.
  Outside this repository, `editors/vscode/scripts/install.mjs` still writes a
  `.vscode/mcp.json` for editors without the extension and for the Copilot CLI;
  running both mechanisms in one workspace would register each server twice.
