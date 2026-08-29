- `binding-audit` no longer reports another worktree's in-flight bindings as
  stale. Bindings live in the repository-shared `spec.db`, but each bound path
  was resolved against whichever working tree the audit ran in — one unchanged
  database reported 4 stale bindings from a checkout at `origin/main` and 0 from
  the worktree holding the branch that adds those files. Acting on the first
  reading would have unbound a peer's unmerged code, landing it ungoverned. A
  path is now stale only when no branch that could still land holds it, and one
  held elsewhere is reported as `IN FLIGHT` with the ref that keeps it alive.
- `binding-audit` now distinguishes a module split from a deletion. When a bound
  `X.rs` becomes `X/mod.rs` plus siblings — which the `rust-module-length`
  control actively asks for — the binding is reported as `SPLIT` with the
  descendants to rebind, instead of as a missing file whose obvious remedy
  (unbind it) is the opposite of the right one.
