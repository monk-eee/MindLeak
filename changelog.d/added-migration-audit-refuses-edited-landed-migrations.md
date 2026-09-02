- `migration-audit --check` now refuses a migration file this branch has edited
  that already exists on `origin/main`, and runs in its own CI job on pull
  requests as well as pushes. `migrate_locked` hashes a migration's whole file,
  so editing one that has already applied — a comment is enough — leaves its key
  held under content no committed source matches, and every `connect()` reaching
  it then refuses. Measured 2026-09-02: splitting an already-applied `0060` in two
  poisoned keys 60 and 61 in `ackplane_test` and cost 58 test failures across five
  unrelated subsystems, none of which named the cause. Nothing caught it — no
  pre-commit hook and no CI job mentioned migrations at all, and the audit only
  ran when someone typed `make migration-audit`. The check asks git for the
  difference (`git diff --name-status`) rather than comparing file text, so a
  Windows checkout whose working copy has CRLF endings does not read as a modified
  migration; a brand-new migration is not flagged, since adding one is the remedy
  this check tells you to use; and a shallow clone that cannot resolve the base ref
  reports that it cannot answer rather than passing quietly.
