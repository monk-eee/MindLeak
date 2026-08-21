### Added

- `canonical-push.mjs` now runs `ingest-agent-memory.mjs` automatically after
  a successful publish, best-effort and never fatal, when the publishing
  agent sets `AGENT_MEMORY_FILE` to its own memory file's path. There is no
  portable default to migrate: agent memory lives wherever the calling
  agent's own tooling keeps it (often outside the repository entirely), never
  at one fixed location every contributor shares -- so this stays opt-in
  rather than assuming a path that would only be correct on one machine.
