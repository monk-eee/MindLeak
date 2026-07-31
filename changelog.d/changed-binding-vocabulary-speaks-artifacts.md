- **The binding vocabulary speaks artifacts, not code.** `link_goal_to_artifact`
  (ADR-0060) writes bindings that can govern any node — `artifact:`, `symbol:`,
  and more — but the type and table it wrote to still said "code", implying the
  store refused non-code nodes when it never did. `CodeBindingMode` and
  `CodeBinding` are now `ArtifactBindingMode` and `ArtifactBinding`, and the
  `goal_code` table is now `goal_artifacts`. Existing ledgers migrate in place on
  first open: every binding, its `mode`, and its index move to the new table.
