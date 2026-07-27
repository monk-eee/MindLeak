- **`CHANGELOG.md` is assembled from per-change fragments (ADR-0056).** A pull
  request now adds `changelog.d/<section>-<slug>.md` and does not touch the
  changelog. The shared file was a serialisation point: `.gitattributes` marks
  it `merge=union`, git honours that in a checkout, and GitHub's merge machinery
  does not — so five pull requests in one day reported a conflict that did not
  exist, `gh pr update-branch` could not clear it because that is a server-side
  merge too, and each one had to be reconciled by hand. The real cost was not the
  conflict but that **auto-merge silently stopped working**: armed work went
  stale the moment anything else landed, which is precisely what "armed means
  finished" was supposed to rule out. Two branches never write the same fragment
  path, so there is nothing to merge. `node scripts/changelog.mjs --release
  <version>` folds the fragments, and anything already under `## [Unreleased]`,
  into a dated section once, in the release commit. This is the same shape as ADR
  numbers and the ADR index table, and the same fix both of those already got:
  stop hand-maintaining what can be computed.
