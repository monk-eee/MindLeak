- A plain `git checkout` no longer snapshots and restores the whole working
  tree. The `post-checkout` hook that records worktree ownership is now
  installed directly by `node scripts/install-hooks.mjs` (run by `make setup`)
  instead of through pre-commit, which wrapped every checkout in a
  discard-and-reapply of the entire tree via a shared user-level cache file.
  Measured on the same single-file checkout: 22 lines of output and a full tree
  snapshot before, 0 lines and none after — for an operation that changes no
  index and makes no commit. `make hook-health` now distinguishes a missing hook
  from one installed by the wrong tool, so an existing clone still carrying
  pre-commit's old shim is reported rather than passing as healthy.
