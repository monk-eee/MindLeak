- **Telemetry now says whether each registered agent session read memory before
  its first attributed write.** `telemetry_snapshot` reports a bounded list of
  the 32 most-recent sessions with successful memory-read and write counts plus
  `yes`, `no`, or `unknown` when no write exists yet. The metric is derived from
  the existing append-only audit trail and scans at most 10,000 recent
  attributed events; no stored verdict or new MCP tool was added. Identity comes
  only from `SessionRegistry`: callers may still use session-less read tools,
  but cannot forge `resolved_agent` to improve the result. Failed reads do not
  count, and opening the same session again starts a fresh observation window.
  This turns memory adoption into an outcome metric rather than treating raw
  recall call volume as proof of a habit.
