- **A pre-push hook-health check verifies the shared hooks are installed.** A
  hook type in `default_install_hook_types` only installs when `pre-commit
  install` is re-run, and the hooks directory is shared across every worktree,
  so a checkout made before `post-commit` was added kept committing with no
  provenance and nothing said so. `scripts/hook-health.mjs` runs from pre-push,
  resolves the hooks directory git will actually use, and refuses the push with
  `pre-commit install --install-hooks` when pre-commit, pre-push, or post-commit
  is absent.
