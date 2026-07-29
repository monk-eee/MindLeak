- **Passive execution evidence depends on VS Code shell integration.** — VS Code
  1.93 shell start/end events provide command/exit evidence; unsupported or
  conflicting shells report degraded capture and are not guessed from terminal
  text. Concurrent terminal executions can both observe one workspace mutation,
  so changed paths prove temporal overlap rather than process-level causality. —
  Medium impact on provenance precision in overlapping command sessions.
