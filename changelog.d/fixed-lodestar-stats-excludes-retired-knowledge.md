- **`lodestar_stats` no longer counts retired knowledge as active.**
  The compact stats surface now uses the same retirement and decay predicate as
  `active_knowledge`, so retiring a lesson removes it from both views
  immediately while preserving its durable history.
