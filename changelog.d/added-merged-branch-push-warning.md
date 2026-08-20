### Added

- `scripts/canonical-push.mjs` now checks the current branch's most recent pull request before pushing and warns (never refuses) when it is already `MERGED`, naming the PR and the `gh pr create --head <branch>` remedy — closes the mechanism gap in `gaps.d/a-branch-whose-pr-merged-can-still-take-commits.md`.
