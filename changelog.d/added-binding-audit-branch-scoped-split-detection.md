### Added

- `binding-audit.mjs --new-since` — the mode `canonical-push.mjs` already runs
  automatically at every push — now also detects a bound module split entirely
  within the pushing branch (a bound `X.rs` replaced by `X/{mod,siblings}.rs`,
  none of which exist on the base ref yet). It reuses the existing `splitInto`
  classification, scoped to the branch's own `git diff --diff-filter=D`
  against its base ref, and prints the descendants to rebind at push time
  instead of only when someone runs a full, unscoped audit by hand.
