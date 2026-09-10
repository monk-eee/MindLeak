- **Previously polluted publication evidence needs an attributed repair path.**
  Observed in `scripts/canonical-push.mjs` / `recordPublication`: publishing
  `0b6860b7b5950d2dd289e108fbb1731ae5511a5e` supplied the cumulative branch diff
  as that nine-file commit's changes. MindLeak `evidence_for` consequently
  attributed branch-wide `refactored` edges to that one intent, making the
  completion bundle for `task:a4bc0fa9102a` untruthful. Fixed this run for future
  publication by reading exact Git commit facts. Existing edges remain: core
  Git ingestion upserts facts, and the installed MCP surface has no targeted,
  provenance-bearing correction operation. Add an auditable correction path
  that verifies commit facts against Git and retracts only erroneous attribution;
  preserve unrelated observations and history. Then repair affected records and
  rerun conformance. The task remains paused rather than certifying a filtered
  or silently rewritten bundle.
