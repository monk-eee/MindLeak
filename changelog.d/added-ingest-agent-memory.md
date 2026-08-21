### Added

- `scripts/ingest-agent-memory.mjs` migrates a flat agent-memory Markdown
  file's per-heading lessons into Lodestar's `record_knowledge`, so a lesson
  that names specific repository files surfaces to whoever next touches
  them instead of sitting inert in a file only the agent that wrote it ever
  rereads -- dogfooding this repository's own durable knowledge store rather
  than accumulating a private notes file indefinitely. `--dry-run` previews
  what would be sent; `--prune` removes only the entries that were actually
  ingested, leaving anything with no extractable repository path untouched
  so nothing is silently lost. Run for real against this session's own
  289 KB memory file: 30 of 176 entries named a repository path, all 30
  ingested cleanly, and the source file shrank to 225 KB.
