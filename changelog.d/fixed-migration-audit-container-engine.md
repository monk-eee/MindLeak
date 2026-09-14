- **Migration audits support the configured container engine:** use
  `node scripts/migration-audit.mjs --engine podman` to read applied migration
  keys and digests from both `ackplane` and `ackplane_test` in Podman. The CLI
  honors `MINDLEAK_COMPOSE_BIN` when no engine is supplied and otherwise retains
  Docker as its default. Unavailable live checks name the selected engine;
  static audit findings and their exit behavior are unchanged.
