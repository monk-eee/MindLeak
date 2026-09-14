- **The migration audit CLI cannot inspect a Podman-only deployment -- OPEN.**
  On 2026-09-14, `scripts/migration-audit.mjs::main` invoked `docker exec` even
  when `--container` named the running, isolated Podman PostgreSQL fixture.
  Docker was unavailable, so both live-database checks were skipped; the source
  checks still passed and correctly reported the missing live evidence. The
  exported `appliedMigrationsFromLiveDatabase` helper worked when supplied a
  Podman runner, and migration 0068's applied digest matched its source.
  Left for later: let the CLI select the configured container engine so operators
  can inspect applied migration identities without an ad hoc runner. No live
  database or container-engine configuration was changed.
