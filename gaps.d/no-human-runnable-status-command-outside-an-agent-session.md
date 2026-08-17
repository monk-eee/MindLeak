- **No human-runnable status command exists outside an agent session.** —
  Observed 2026-08-17: every number a person has about live Lodestar/MindLeak
  state (`lodestar_stats`, `fleet_view`, `telemetry_snapshot`, board health)
  comes from an agent calling MCP tools mid-conversation and relaying the
  result; there is no equivalent a person can run directly from a terminal.
  Where: `scripts/board-health.mjs` and `scripts/stranded-report.mjs` come
  close but read only the Lodestar side and require the caller to already
  know which script answers which question. Impact: a person who wants a
  quick sanity check of "is anything stuck right now" has to open an agent
  session and ask it to relay tool output, which is slower and adds an LLM
  round trip to what is a deterministic local read. Left for later: needs a
  single `node scripts/status.mjs` (or `make status`) that prints the
  equivalent of `lodestar_stats` + `fleet_view` + `telemetry_snapshot`'s
  health summary directly against the local `spec.db`/`graph.db`, no MCP
  server or model involved; not built this run.
