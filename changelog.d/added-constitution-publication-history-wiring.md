### Added

- Ackplane's `PublishConstitutionSnapshot` RPC now records an immutable
  publication history entry (ADR-0121 decision 1) alongside the mutable
  active-snapshot replace it already performed, closing decision 2's
  authenticated-publish-path requirement. The immutable record is checked
  first: a republish of the same `version_id` with different content is
  refused before the active snapshot ever changes, so a rejected republish
  never silently moves the active pointer. New optional wire fields
  `schema_version`, `source_ref`, and `content_digest` carry the publication's
  own schema version and optional source provenance.
