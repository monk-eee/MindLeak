- Each window now roots its MCP servers at the worktree it is editing (ADR-0073).
  `.vscode/mcp.json` bound both servers to `${workspaceFolder}/target/release`,
  and every window's workspace folder was the primary checkout, so a file edited
  in any other worktree could not be made repository-relative. Measured on
  2026-07-30: `ingest_file` refused 257 of 6450 calls (4.0%), roughly two per
  minute, and those files never entered the graph at all.
  `cwd` and `MINDLEAK_WORKSPACE` still follow `${workspaceFolder}`, so opening
  the worktree as the workspace folder is now enough to make saves land under the
  canonical id. Worktrees continue to share one graph and one board — the
  repository id derives from the git common dir, not from the folder opened,
  which was verified across three worktrees before the change.
  The servers are installed once per machine at `~/.mindleak/bin` by
  `make install-servers`, rather than being built into all 56 worktrees (184 GB
  of build output already on disk, only 15 holding a server binary). Because the
  binary now sits outside the workspace, the build notice reports it as an
  installed binary — identity without a staleness claim it cannot support.
