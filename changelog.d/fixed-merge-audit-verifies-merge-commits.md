- `merge-audit` now verifies that every merge commit on `main` is actually the
  merge of its own two parents, failing CI when a merge silently dropped one
  side's work. This catches the PR #507 failure — a merge that reported clean
  while its tree kept only one side — retrospectively and regardless of which
  button made the merge, where the existing guards could only check the one
  merge they were about to make themselves. It also works after the branch is
  deleted, unlike the `git cherry` half of the same audit, and that half now
  reports how many branches it could not check instead of printing only the
  ones it managed.
