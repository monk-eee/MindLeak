- **Some server library fixtures depend on telemetry schema created elsewhere.**
  On an isolated PostgreSQL database initialized only by the claim tests,
  `cargo test -p ackplane-server --lib` returned 662 passed and eight failed:
  seven `administration_store::purge_tests` fixtures and
  `export_provider::tests::create_telemetry_export_redacts_internal_identifiers_and_bounds_records`
  failed with `relation "telemetry_events" does not exist`. Running the normal
  `migrate` binary first made all 670 library tests pass; the full industrial
  runner also passed. Left for later: make those standalone fixtures establish
  their own telemetry schema dependency instead of relying on unrelated tests
  or prior suite setup. This is independent of the node-custody security fix.
