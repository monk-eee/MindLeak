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
  hid: the split between what enforces and what does not. Measured again after
  the fleet-discipline clauses were adopted, that split is **thirty active
  clauses, thirteen carrying a complete contract, and four binding any control
  at all — two of them mechanical**. The four are the source-file length
  ratchet and commit-provenance ingestion (both `observed`), and the
  shell-plumbing and worktree-ownership hooks (both `mechanical`).
  The earlier reading of this fragment — that six workflow rules bound
  mechanical controls — no longer holds, and the reason is worth knowing rather
  than quietly restating a number. Those delivery clauses were amended, and an
  amendment used to leave its controls pointing at the superseded clause id, so
  they were orphaned: `clause_controls` now reports `one-publishing-owner-per-task-branch`
  and `a-commit-stays-inside-its-declared-scope` as unguarded, though both still
  declare `block`. The mechanisms themselves never stopped working — the
  pre-commit hooks still exit non-zero — but the ledger can no longer resolve
  those clauses above `advise`, which is precisely the "a control that has
  stopped enforcing reads exactly like one that works" failure. Carrying active
  controls across an amendment by slug fixes it going forward and re-adopts the
  stranded ones at the next amendment; until then the count above is the honest
  one.
  The clauses that enforce nothing still include every locally-migrated clause,
  which is to say every invariant the project wrote about itself: the zero-token
  hot path, decay, derived effective weight, the local-only security boundary.
  Borrowed rules about how work is delivered are the ones that acquired
  mechanisms; the project's own rules about what it must never do have none.
  That much is correct behaviour — migration invents no authority (§10) and
  broad principles route to review (§13) — and it was invisible while the export
  rendered enforcing and inert clauses identically.
