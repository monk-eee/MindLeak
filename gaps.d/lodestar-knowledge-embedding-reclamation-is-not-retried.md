- **Lodestar recorded the knowledge-embeddings cascade migration before its
  best-effort physical cleanup -- VERIFIED 2026-08-27, repair in progress.**
  `db::configure` only ran `VACUUM` for the opener that rebuilt the table; a
  lock conflict, crash, or database upgraded before the cleanup release left
  free pages permanently allocated because the schema migration would never run
  again. The successful vacuum also retained its allocation in `spec.db-wal`,
  so the derived-vector cleanup could leave the same disk footprint it was
  intended to reclaim.
