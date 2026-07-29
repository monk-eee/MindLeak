- **The Design Board silently swallowed a cancelled materialization, and planned
  from an empty summary — FIXED.** — `promote` / `revisePromotion` returned with
  no message, no log line, and no state change whenever any quick pick or input
  box was dismissed, so an accepted design simply stayed `pending` and a
  cancelled run was indistinguishable from a broken one; ADR-0033 sat that way
  for three days and read as an unusable tool. Separately, `parseAdrMetadata`
  hardcoded `summary: ""`, so Create-mode `plan_design_promotion` saw only the
  ADR title and drafted generic filler ("Review documentation", "Design a
  workflow model") — the same shape as the earlier hallucinated-task incident,
  and a direct route back into the ADR-0028 duplicate/orphan failure. — Medium
  impact: no data loss, but the Design Board was effectively unusable and its
  planning output untrustworthy. — Fixed Jul 2026: every abort path reports and
  logs, an empty objective list explains itself instead of closing, and
  `extractAdrSummary` carries bounded `## Decision` + `## Context` text into the
  design item planning reads. The store half mattered just as much:
  `reconcile_design_item` used `INSERT OR IGNORE`, so no repository pass could
  ever repair an already-registered empty summary; it now refreshes `title` and
  `summary` while leaving status, decision, proposer, and promotion state
  durable. Note the extension half is TypeScript, so an **installed**
  extension keeps the old behaviour until it is rebuilt and reloaded.
