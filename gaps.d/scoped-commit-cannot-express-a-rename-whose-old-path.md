- **`scoped-commit.mjs` cannot commit a staged rename whose old path isn't
  also in the declared path list -- OBSERVED 2026-08-21, OPEN.**
  `git commit -- <pathspec>` builds its commit tree from INDEX content for
  paths named in the pathspec and falls back to HEAD content for every path
  NOT named -- including a path that was DELETED in the index. Splitting
  `crates/lodestar-mcp/src/tools/executive.rs` into
  `executive/{mod,constants,definitions,claim,tasks,render}.rs` via `git mv`
  (fully and correctly staged, verified via `git write-tree`/`git ls-tree`)
  still failed `cargo-fmt`/`cargo-clippy` pre-commit hooks with `file for
  module executive found at both executive.rs and executive/mod.rs`, even
  though the real index had no `executive.rs` entry at all. Root cause
  confirmed by temporarily instrumenting pre-commit's own
  `staged_files_only.py` (site-packages, not repo-tracked) to dump its
  internal `git diff-index` patch: it showed a genuine `deleted file mode`
  diff for `executive.rs` -- proof that `git commit -- <new-path-only>`
  reconstructed a commit-time tree that silently kept HEAD's version of the
  old file alongside the new one.

  `scoped-commit.mjs` cannot express the fix today: its single `paths` array
  feeds both the `git add -- <paths>` call (which refuses a pathspec
  matching a nonexistent file -- `git add` needs a file to add, not just a
  pathspec) and the final `git commit -- <paths>` call (which DOES need the
  old, deleted path listed to apply the deletion correctly). The workaround
  used this time: stage everything via the script's own failed `add` step
  (harmless -- the rename/new files were already staged from an earlier
  `git mv`/edit), then run `git commit -F <msgfile> -- <old-deleted-path>
  <new-paths...>` directly. All the same hooks still ran for real (fmt,
  clippy, worktree-ownership, gap-fragments, commit-as-evidence) -- nothing
  was bypassed, only the path list passed to the final commit differed.

  Fix direction: teach `scoped-commit.mjs` to detect staged renames
  (`git diff --cached --name-status -M -- <declared paths>`) before the
  `git add`/`git commit` calls, and automatically include the old side of
  any rename in the final `git commit -- <pathspec>` list (never in the
  `git add` list, since that side no longer exists on disk). Silent failure
  mode if this ships unfixed: a rename with no module-name collision to
  surface it (e.g. a plain content rename, not a Rust `mod` ambiguity) could
  commit successfully while quietly retaining the stale old file's HEAD
  content in the tree, with no error at all.
