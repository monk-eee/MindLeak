### Added

- **`ACKPLANE_SINGLE_TENANT_ATTESTED` gates production recovery execution
  (ADR-0145 decision 6, slice 3).** Restoring a platform Snapshot replaces the
  whole database, so on a multi-tenant deployment it would overwrite every
  other tenant's data. Execution is now refused with a typed
  `SnapshotProviderError::MultiTenantRecoveryUnavailable` unless an operator
  has set this to exactly `true` — which is every deployment until someone
  explicitly does, by design.
- The attestation is **never inferred** from how many tenants a deployment
  currently has. A platform holding one tenant today can onboard a second
  tomorrow, so this is a durable statement about the deployment's shape rather
  than a runtime headcount. Only an exact `true` (case-insensitive, trimmed)
  attests: `1`, `yes`, `on` and a malformed value all leave it unset, because
  reading a typo as an attestation destroys other tenants' data while reading
  it as absent only refuses a capability the operator can re-enable.
- Recovery *rehearsal* is deliberately not gated by this. A rehearsal restores
  into an isolated scratch database (`ACKPLANE_REHEARSAL_DATABASE_URL`) and is
  useful on every deployment shape.
