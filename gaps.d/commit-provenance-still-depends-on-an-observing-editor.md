- **Commit provenance still depends on an observing editor because the
  post-commit hook is not installed — PARTIALLY FIXED.** The earlier version of
  this gap said commits landed with no provenance at all. That was false: the
  VS Code extension's passive Git sensor (`editors/vscode/src/gitSensor.ts`)
  watches `repository.state.HEAD` and calls `ingest_commit` with the full commit
  hash and the commit's own date. It explains why provenance appeared with no
  hook and why re-ingesting some commits returned `nodes_created: 0`. Two prior
  diagnoses were also false and must not be revived: the missing hook did not
  run and time out, and `canonical-push` contains no ingest call.

  The sensor now distinguishes Git operations rather than relying on ancestry
  alone. A checkout is never attributed as work, including a checkout to a
  descendant branch whose tip names the previous HEAD as a parent. The first
  real commit after that checkout is captured. An explicit non-linear commit
  (for example amend) upgrades an in-flight state refresh instead of being lost
  behind the duplicate-flight guard. A state-only non-linear move remains
  un-attributed because it may be reset, rebase, or external checkout; guessing
  would attach old history to whoever happened to view it.

  What remains is coverage outside an observing editor. `.pre-commit-config.yaml`
  declares `post-commit`, but the shared `.git/hooks` directory was installed
  before that declaration and contains only `pre-commit` and `pre-push`.
  `default_install_hook_types` takes effect only when `pre-commit install` is
  run again. An agent committing in a worktree that no VS Code window has open
  therefore records nothing, silently. Installing the hook is a fleet-wide load
  change: the hooks directory is shared by every worktree and each commit would
  spawn an MCP server, so it needs an operator decision rather than one agent
  changing the running fleet underneath everyone else.

  The sensor regression is covered in both directions: with the fix removed,
  four of eight focused tests fail; restored, all eight pass. The remaining
  editor-dependence is not fixed by that change and stays public here.
