- **`db::configure` only attempted an embeddings-table `VACUUM` in the process
  that rebuilt the table, and did not truncate its WAL -- VERIFIED 2026-08-27,
  repair in progress.** The rebuild transaction commits before the best-effort
  vacuum, so a crash, lock conflict, or database upgraded before the reclamation
  release leaves its free pages permanently allocated. A successful vacuum also
  leaves its allocation in `graph.db-wal` for the lifetime of the MCP
  connection. The data remains correct, but the cascade migration advertised as
  reclaiming orphaned-vector storage can leave the database consuming the same
  disk space it did before the repair.
