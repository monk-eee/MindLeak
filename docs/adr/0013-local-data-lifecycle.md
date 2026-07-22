# ADR-0013: Local data backup, export, and reset lifecycle

- Status: Accepted
- Date: 2026-07-22
- Deciders: MindLeak maintainers
- Related: [ADR-0004](0004-intent-plane-spec-brain.md),
  [ADR-0010](0010-observability-and-resilience.md)

## Context

Product installation and upgrades need a rollback path. Copying an active SQLite
file in WAL mode can produce an inconsistent backup, while deleting database
files under running MCP processes leaves open connections and sidecar files.
MindLeak also has two stores with different semantics: the memory plane is
regenerable; the Lodestar constitution and task ledger are durable intent.

A graph JSON export is useful for inspection and portability but is not a full
backup: it omits SQLite implementation state and cannot preserve every audit or
index table. Operators need both concepts and they must not be conflated.

## Decision

- Add a shared `mindleak-storage` crate using SQLite's online backup API. Backups
  write to a temporary sibling, run `PRAGMA integrity_check`, and atomically
  rename into a destination that must not already exist.
- Expose `backup_database(path)` from both MCP servers. The resulting files are
  complete plane-specific SQLite backups, including telemetry/derived tables.
- Do not expose live restore. To restore, stop all clients/servers, preserve the
  current file, replace it with a verified backup, then restart so normal schema
  migration runs once. Downgrade across schema changes is unsupported.
- Expose `export_graph()` from MindLeak as human-readable JSON containing all
  nodes and currently active, fully derived weighted edges. Export is not backup.
- Expose separate destructive reset operations:
  - MindLeak requires exact token `RESET MINDLEAK` and clears graph nodes/edges,
    unresolved stubs, embeddings, and telemetry.
  - Lodestar requires exact token `RESET LODESTAR` and clears goals, task/lease
    state, code bindings, conformance audit, and learned knowledge.
- Reset works through transactions on the live connection; database files and
  schema remain valid. Resetting one plane never touches the other.
- The VS Code extension offers Backup Both, Export Graph, and Reset Memory.
  Durable intent reset remains an explicit MCP/operator action, not a toolbar
  convenience.

## Consequences

- Upgrades have a consistent, cross-platform backup and rollback workflow while
  servers are running.
- Destructive operations are deliberate and auditable; accidental cross-plane
  deletion is structurally prevented.
- Backups may contain source excerpts, terminal output, commit messages, goals,
  and audit events. They require the same filesystem protection as the workspace.
- JSON export can be reviewed or moved between tools, but restoration uses the
  SQLite backup, not the JSON document.
