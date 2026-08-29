- `scoped-commit` no longer fails when a declared path is the old side of a
  staged rename. Naming both sides of a file-to-module-directory split (for
  example `git mv daemon.rs daemon/mod.rs`, then committing both paths) aborted
  staging with `fatal: pathspec ... did not match any files` and exit 128, for a
  rename already correctly in the index — the skip-list read `--diff-filter=D`,
  which never lists a rename's old side because git reports it as `R`.
- `scoped-commit` now warns, before creating the commit, when no live claim of
  this session covers it. `check_conformance` bounds evidence by the claim that
  authorised it, so a commit made outside a claim window can never be certified
  and claiming afterwards does not reach back over it. The existing warning fired
  at publish time, by which point the only remaining moves are `merge_evidence`
  or a human resolve; this one arrives while stopping and claiming is still an
  option. It never blocks the commit, and an unreadable ledger is reported as
  unreadable rather than passed over in silence.
