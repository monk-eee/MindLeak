- **Lodestar task recovery and retirement verbs.** — `reopen_task` returns a task
  stranded in `in_review` or a manual `blocked` hold to claimable `open`, and
  `abandon_task` retires a nonterminal task to terminal `abandoned` (facade + MCP
  tool, regression-tested), making `TaskStatus::Abandoned` reachable and closing
  the retire-a-mis-filed-task gap. — Resolved Jul 2026. Note: the verbs are wired
  in source, but a stale running MCP binary may not expose them until
  rebuilt/restarted (see the stale-binary gap above).
