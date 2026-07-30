- **The Design Board reads the ADR record from main, not from the open
  checkout.** `readWorkspaceAdrMetadata` globbed `docs/adr` on disk and
  registered whatever it found, so the decisions the ledger was told about
  depended on which worktree the window happened to have open. Under ADR-0038
  that is a different subset per window: measured across 84 attached worktrees,
  `origin/main` held 75 ADRs and the union across all 196 remote branches was
  also 75 — main is the complete record and nothing is ever branch-only — yet 65
  worktrees were missing between 1 and 26, and the checkout this extension reads
  held **49 of 75**. Registering that subset tells the ledger that 26 decisions
  do not exist, silently. The record now comes from `origin/main` through a
  single `git cat-file --batch`, one process rather than one per ADR. A folder
  whose ref cannot be resolved — a fresh clone with no remote — still falls back
  to its working tree, but says so in the output channel and a warning, because
  a partial record that reports itself as complete is indistinguishable from a
  good one. The git read lives in `adrRecord.ts`, free of `vscode` so it is
  directly testable; its content is parsed by byte length, since git's framing
  counts bytes and one multi-byte character would otherwise shift every ADR
  after it.
