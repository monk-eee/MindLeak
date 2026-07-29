- One-off data repairs move out of `db/migrations.rs` into `db/repairs.rs`. A
  schema migration changes shape and is cheap to re-run; a repair rewrites rows
  to undo damage a defect already did, and firing twice can undo work someone
  did in between. Filing them together made that distinction invisible. The
  split also returns `migrations.rs` below the module-length clause: adding the
  stranded-clause repair had pushed it from roughly 416 to 476 non-test lines,
  past the 450 the clause allows, taking the repository from 7 oversized modules
  to 8. It is back to 7.
