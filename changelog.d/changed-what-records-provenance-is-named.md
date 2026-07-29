- **Named what actually records commit provenance, and what it silently skips.**
  A Known gap recorded the question as unexplained: commits were getting
  provenance with no post-commit hook installed, and two guesses at the cause —
  that the hook ran and timed out, and that `canonical-push` ingests — were both
  wrong. The answer is the VS Code extension's passive git sensor:
  `editors/vscode/src/gitSensor.ts` watches `repository.state.HEAD` and calls
  `ingest_commit` with `commit.hash` and the commit's own date. That also
  explains why re-ingesting a commit by hand returns `nodes_created: 0` — the
  sensor got there first.
  The more useful half is what it does *not* record. It ingests only when the new
  HEAD is a child of the previous one, so a branch switch, a checkout, or any
  non-linear HEAD move records nothing. In a fleet where every unit of work gets
  its own branch that is the common path rather than an edge case, and it is the
  true cause of the empty evidence bundles that were repeatedly read as
  ingestion being broken. Provenance also depends on the workspace being open in
  an editor at all: an agent working through a terminal in a worktree nobody has
  open records nothing, silently.
  Documentation only. Whether that skip is the right behaviour is a separate
  decision and is deliberately not settled here.
