- **`worktree-reclaim` refuses loudly when the Lodestar task board cannot be
  read, instead of silently keeping every named worktree.** A stale server
  binary or an unreadable board previously produced `keeping every named
  worktree`, which reads as caution and is never investigated — the tool
  appears to be working while reclaiming nothing.
  `worktree-reclaim.mjs` now exports `claimStateRefusal`, which names the
  actual cause and, when the board specifically could not be parsed, reuses
  `claim-gate.mjs`'s shared `unreadableBoardGuidance` to point
  `LODESTAR_MCP_BIN` at the current shared install.
