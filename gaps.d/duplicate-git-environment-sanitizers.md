- **The ADR guard duplicates the shared Git-environment sanitizer.**
  `scripts/adr-guard.mjs::gitEnvironment` maintains the same six repository
  variables and copy/delete logic as `scripts/adr-files.mjs::gitEnvironment`.
  A future isolation change can leave the guard and other Git readers behaving
  differently. Consolidate the guard onto the shared helper with its existing
  guard tests; left for a separate scoped follow-up, not changed by the archive
  provenance repair.
