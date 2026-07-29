- **ADRs with a qualified status were dropped from the ledger in silence —
  FIXED.** — Observed Jul 2026. `parseAdrMetadata`
  ([`editors/vscode/src/designBoard.ts`](editors/vscode/src/designBoard.ts))
  required the status line to equal `proposed`/`accepted`/`rejected` exactly, so
  `Accepted (implemented)` and `Accepted (no symbol-lease primitive)` failed the
  check and returned `null`. ADR-0015 and ADR-0017 were therefore never
  registered at all, while `sync()` kept logging success with a lower count, so
  nothing reported the loss. — High impact: an accepted decision invisible to the
  design ledger is exactly the failure this ledger exists to prevent. — Fixed
  this run: `normalizeAdrStatus` strips a parenthetical qualifier, and
  `readWorkspaceAdrMetadata` now returns the skipped paths with a reason which
  `sync()` logs and warns on. Regression test: "accepts a status carrying a
  parenthetical qualifier".
