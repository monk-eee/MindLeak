### Added

- Bridge repository knowledge now uses ADR-0112 keyset pagination instead of
  silently stopping after 50 active records. Callers pass
  `before_confirmed_at_micros` and `before_knowledge_id` together, then follow
  `next_before` until it is `null`. Ordering by confirmation time and
  knowledge id keeps page boundaries stable when entries share a timestamp,
  without changing the knowledge domain's semantic recall ranking.
