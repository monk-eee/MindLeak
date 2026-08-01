- **Knowledge can be searched by meaning, not just by spelling.** `active_knowledge`
  gains an optional `query` that ranks lessons by semantic similarity, so a lesson
  reaches the agent asking the question it answers rather than only the one who
  already guessed its wording. It sits behind the read agents already perform —
  there is no new verb to remember (ADR-0080). Embedding is optional and
  best-effort: recording a lesson never depends on a model being reachable, and
  when none is, the reply degrades to substring matching and says which mode
  answered in `match_mode`. The existing `contains` filter keeps its exact
  substring contract unchanged. Configure with `LODESTAR_EMBED_URL` /
  `LODESTAR_EMBED_MODEL` (defaults: a local Ollama, `nomic-embed-text`).
