- The editor no longer watches or searches build output, which is what made this
  repository slow to work in. `files.watcherExclude` and `search.exclude` were
  absent from the committed `.vscode/settings.json` and from user settings, so
  VS Code watched and indexed everything under every open workspace folder.
  Measured 2026-07-30: 88 worktrees, 86 carrying a `target/` directory, 61
  carrying `editors/vscode/node_modules`, and one sampled `target/` holding
  82,891 entries — on the order of seven million watched files that nobody ever
  edits. At the same moment: 7.0 GB free of 55.6 GB, 39 VS Code processes
  holding 17.1 GB across 8 renderers and 10 utility processes, CPU at 52% of 16
  cores. Every `cargo build` rewrites thousands of files inside a watched tree,
  and every window whose workspace contains it is notified.
  The MCP servers were measured too and are not the cause — four processes,
  55 MB, under 40 seconds of CPU between them. Recorded because the obvious
  suspect was wrong, and the measurement is what said so.
  `target`, `node_modules`, `.vscode-test`, `out`, `dist`, `coverage` and the
  local state directories are now excluded from watching and from search. The
  settings file is tracked, so every worktree and every fresh clone inherits it
  rather than each machine being configured by hand.
  `files.exclude` is deliberately not set: it hides entries from the explorer
  rather than reducing work, and hiding a directory someone may need to open is
  a real cost for no measured gain.
  Takes effect for a window when that window reloads. It does not shrink the 88
  worktrees or the disk they occupy, which is a larger and separate problem.
