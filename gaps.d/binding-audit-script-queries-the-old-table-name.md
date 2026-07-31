- **`scripts/binding-audit.mjs` still queries the pre-rename table name —
  OPEN, follow-up to the goal_code → goal_artifacts rename.** The audit script
  reads `select goal_id, node_id, mode from goal_code`, but that table was
  renamed to `goal_artifacts` (`refactor/binding-vocabulary-speaks-artifacts`).
  It was left untouched deliberately: PR #318
  (`feat/unbound-files-reported-at-publication`) is concurrently editing the same
  file, and editing a file another PR owns is the collision this fleet avoids.
  Impact: the script fails with `no such table: goal_code` once the rename
  merges, until its query is updated. Fix: change the query to `goal_artifacts`
  in whichever of the two branches lands second.
