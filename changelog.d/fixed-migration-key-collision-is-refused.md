- A migration whose key another branch already applied is now refused loudly
  instead of being silently skipped. `ackplane_schema_migrations` recorded only
  the key, so `migrate_locked` could not tell "this migration already ran" from
  "someone else's migration holds this number" — it skipped the DDL and returned
  success, leaving the schema without whatever that migration creates and
  surfacing much later as an unrelated missing relation. The ledger now records
  each migration's content digest, and a key held under different content fails
  with a message naming both digests and the remedy
  (`migration-audit.mjs --next`). Re-running the identical migration still skips,
  as before. A row applied before the digest column existed says nothing about
  which migration wrote it, so it is skipped exactly as it always was and its
  digest adopted, never assumed.
