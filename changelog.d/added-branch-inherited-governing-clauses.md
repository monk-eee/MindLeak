- Implemented ADR-0147: `task_claim(step="claim")` accepts an optional
  `branch_committed_paths` (workspace-relative paths already committed on this
  branch since its base) and reports `branch_inherited` — the active clauses
  governing those paths that are not already covered by the task's own
  declared scope or its `also_serves` coverage — separately from `governing`.
  Advisory only, never a gate, and absent entirely when the argument is
  omitted or empty, so a caller that ignores it sees exactly today's response.
  Wiring a caller (`scripts/canonical-push.mjs`/`scripts/mcp-direct.mjs`) to
  compute and supply the diff before claiming is separate follow-up work, not
  part of this change.
