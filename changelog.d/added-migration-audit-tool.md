- **`scripts/migration-audit.mjs` (`make migration-audit`) reports
  `ackplane-server` migration-key defects.** Two of this repo's own migration
  numbers (19, 27) have reached the shared development Postgres from
  concurrent, not-yet-committed work with no corresponding committed file,
  silently poisoning the key for the branch that later committed it
  (`gaps.d/unaccepted-work-migration-reaches-shared-db.md`). This reports the
  same class of defect on demand instead of by accident: two committed
  constants naming the same key, a committed constant or `migrations/*.sql`
  file with no match on the other side (both static, no database needed), and
  — when the shared dev container is reachable — keys the live ledger has
  applied that this branch's own source never declared. `--next` folds all
  three sources together to print the one key actually safe to assign next,
  which is what would have prevented both prior collisions in the first
  place: each was a branch computing "next available" from committed `main`
  alone, blind to what a concurrent branch had already applied.
