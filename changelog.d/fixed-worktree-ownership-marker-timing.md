- **A freshly created worktree now records its owner immediately, not only on
  first commit.** `git worktree add` completes its own checkout, and the
  `post-checkout` hook that fires there had no equivalent to the `pre-commit`
  ownership check — a linked worktree that had been created and declared, but
  never committed in, carried no marker and read as unclaimed to any guard
  that consulted one. A concurrent session could remove it, recreate the same
  path on its own branch, and silently receive the first session's next
  edits. `worktree-owner.mjs` is now also wired to `post-checkout`
  (`default_install_hook_types` and `hook-health.mjs`'s expected set both
  gained it), reusing its unchanged ownership logic to write the marker the
  moment the worktree exists.
