### Fixed

- **One push now runs both test runners.** `scripts/*.test.mjs` are `node:test`
  suites and ran before every push; `editors/vscode/scripts/*.test.mjs` are
  vitest suites importing the very same modules through `../../../scripts/`, and
  ran only in CI. So renaming an export or a guidance string passed every local
  check and failed after publishing, on assertions the author had no reason to
  run.

  It was not hypothetical: three pull requests were blocked on exactly this at
  once — `droppedCommits` → `classifyCommits` in the merge audit, and
  `claim_task` → `task_claim` in the claim gate — each author discovering their
  own rename from a red build, while `main` stayed red behind them.

  Measured against the real rename: `script-tests` reports **139 passed, 0
  failed**, completely blind to it, while the hook reproduces CI's failure
  exactly, down to the assertion text, in **12 seconds**.

  Targeted rather than wholesale, and the difference matters. The full extension
  suite takes ~120s here and reports vitest worker timeouts under fleet load; a
  gate that intermittently blocks a push teaches people to reach for
  `--no-verify`, which is worse than no gate. The hook receives the changed
  files and runs only the suite covering a module that actually changed, so a
  Rust or docs push pays nothing — and says so, rather than passing in silence.

  It refuses rather than skipping when the extension's dependencies are absent:
  a silent skip is indistinguishable from a green suite, which is the failure
  this exists to prevent.
