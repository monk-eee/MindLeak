- **A ledger-only task has no mutation evidence and therefore routes to human
  review - OPEN, narrowed 2026-07-31.** The original gap also covered normal
  code work whose agent forgot to call `ingest_commit`. That path is now closed:
  the shared `post-commit` hook records commits, `scripts/hook-health.mjs`
  verifies the hook is installed before push, and canonical publication records
  the published head as a second deterministic path.

  What remains has the opposite cause. `design_register`, decision attribution,
  supersession, waiver grants, and task resolution mutate only Lodestar's
  durable ledger. They create no commit, execution, or changed MindLeak node, so
  `evidence_for` is empty because the act genuinely changed no repository
  artifact. Measured on `task:680b14565a8f`: registering ADR-0073 produced
  check 369 `needs_human` with `evidence contains no provenance-bearing
  mutation`, and human resolution was the only honest terminus.

  Do not manufacture a file edit to clear this result; that launders a ledger
  act as code evidence. Closing the residual requires an explicit design for a
  first-class, attributable ledger-act evidence kind. Until then, human review
  is correct for these uncommon authority-bearing operations.
