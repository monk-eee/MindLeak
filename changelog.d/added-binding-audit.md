- `scripts/binding-audit.mjs` reports Lodestar goal/code binding coverage: source
  files no goal binds, bindings naming a path that no longer exists, and
  bindings stranded on superseded goals. `--check` exits non-zero on the first
  two, so it can gate CI. Cross-platform, read-only, no model.
