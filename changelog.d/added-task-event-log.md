- **The task lifecycle now has an append-only log (`task_events`), seeded with
  the present.** ADR-0064's first step: the table, the `TaskEvent` model, and a
  once-per-database genesis import. Each event carries the full after-image of
  the task the transition produced, so replaying the log is a deterministic
  assignment rather than a re-derivation that could drift from the guarded
  UPDATE it mirrors. `append` takes a connection rather than `&self`
  specifically so callers pass the transaction already open for the state
  write — an event committed separately from the row it describes could exist
  without it, and the projection would stop being checkable.
  Two deliberate choices. There is **no** foreign key to `tasks`:
  `task_claim_transfers` cascades on delete, which is right for an audit of a
  row that must exist, but a record of what happened must outlive its subject
  rather than vanish with it. And the genesis import writes state only, with no
  invented history before it — the claims and lapses that produced each current
  row were never recorded, and manufacturing plausible ones would put fiction
  in an audit ledger. Per ADR-0063 it is registered by name in
  `schema_migrations` and touches no task row, so no live claim moves.
  Nothing emits events yet; the write path is unchanged.
