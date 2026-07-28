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
  hid: the split between what enforces and what does not. Seven clauses carry a
  complete contract and six bind a mechanical control — all of them adopted
  workflow rules, scoped `workflow:git.*` and `workflow:topology`. The eighteen
  that enforce nothing include every locally-migrated clause, which is to say
  every invariant the project wrote about itself: the zero-token hot path, decay,
  derived effective weight, the local-only security boundary. Borrowed rules
  about how work is delivered are enforced; the project's own rules about what it
  must never do are not. That is correct behaviour — migration invents no
  authority (§10) and broad principles route to review (§13) — and it was
  invisible while the export rendered enforcing and inert clauses identically.
