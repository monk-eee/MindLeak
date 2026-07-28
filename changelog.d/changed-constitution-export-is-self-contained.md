- **The constitution export is self-contained, so policy can actually be audited
  from it (SPEC-CONSTITUTION §13).** It rendered clause statements grouped by
  kind and nothing else: a reviewer handed the file could not tell which
  constitutional version it was, where a clause came from, what mechanically
  enforced it, or which exceptions were live — the four things an audit consists
  of. It now carries a `## Version` section (id, version, status, created and
  activated attribution, project identity, purpose, preamble), per-clause
  provenance, declared consequence, waivability and bound controls, and a
  `## Active waivers` section. Absent values render as `_not recorded_` rather
  than being omitted, because the migration that creates the first version
  deliberately invents neither rationale nor authority, and a document that drops
  its empty fields disguises that as completeness. On this repository the export
  grew from 7,133 to 10,674 bytes and immediately showed something the old one
  hid: all 25 migrated clauses declare no consequence and bind no control, so
  they are inert rather than enforcing.
