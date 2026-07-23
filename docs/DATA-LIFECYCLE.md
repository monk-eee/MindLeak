# Local Data Lifecycle

MindLeak keeps two independent SQLite databases. Treat them differently:

| Plane | Default path | Contents | Recovery posture |
|---|---|---|---|
| Memory | `.mindleak/graph.db` | code structure, executions, commits, optional embeddings, tool telemetry | Regenerable, decay-weighted |
| Intent | `.lodestar/spec.db` | constitution, tasks/leases, code bindings, conformance audit, learned knowledge | Durable; back up before upgrade |

Both files can contain workspace-sensitive source excerpts, commands, terminal
output, commit messages, goals, and audit events. Store backups with the same
filesystem protection as the workspace. Neither server opens a network listener.

## Backup

Call `backup_database(path)` on each server. It uses SQLite's online backup API,
checks the resulting database with `PRAGMA integrity_check`, and only then makes
the destination visible. The destination must not already exist.

Use distinct destination names, for example:

```text
mindleak.backup_database(path = "/safe/place/mindleak-before-v0.2.0.db")
lodestar.backup_database(path = "/safe/place/lodestar-before-v0.2.0.db")
```

A backup is a complete copy of one plane, including audit and derived tables.
Back up both planes for a complete workspace recovery point.

## Upgrade and rollback

1. While the current servers are running, create and retain both backups.
2. Stop every client that has either MCP server open.
3. Install the new binaries and restart the client. Schema migrations run when
   each server opens its database.
4. Verify `graph_stats`, `lodestar_stats`, and `get_constitution` before deleting
   old binaries or backups.

For rollback, stop every client first. Preserve the post-upgrade databases for
diagnosis, restore both backup files to their configured paths, reinstall the
matching older binaries, and restart. Live restore and schema downgrade are not
supported. Do not copy only the main database file while a server is running;
WAL sidecars may contain committed data.

## Export

`export_graph()` returns all MindLeak nodes and all currently active edges with
their derived decay and signal weights. It is suitable for review, analysis, or
portable JSON capture. It deliberately omits implementation tables and cannot be
used to restore a database.

`export_constitution(path?)` renders the active Lodestar constitution as
committable Markdown. It does not include tasks, leases, audits, or knowledge and
is likewise not a complete backup.

## Reset

Reset is destructive and plane-specific:

```text
mindleak.reset_database(confirm = "RESET MINDLEAK")
lodestar.reset_database(confirm = "RESET LODESTAR")
```

The MindLeak reset clears graph nodes/edges, unresolved stubs, embeddings, and
telemetry. It never touches Lodestar. The Lodestar reset clears the constitution,
tasks/leases, code bindings, conformance audit, and learned knowledge. It never
touches MindLeak. Back up first unless deliberate unrecoverable deletion is the
goal.

The database files, schemas, and running connections remain valid after reset.

## Retention and privacy

- Raw failure/mutation evidence has a 24-hour default half-life; structural and
  intent-linked code edges use 168 hours; weak association and agent attention
  use 48 hours. Teams may commit bounded overrides in `.mindleak.toml` (ADR-0014).
  Effective weight is derived at query time; changing policy does not rewrite
  database rows.
- `prune_graph` removes evidence below the active threshold after surfacing
  proven near-expiry signal for optional consolidation. `prune_knowledge`
  removes unconfirmed Lodestar knowledge below its longer-lived threshold.
- MCP tool and autonomous maintenance telemetry form an append-only local audit
  and do not decay. Maintenance events contain counts/coarse outcomes, not model
  inputs or responses. Reset the memory plane to erase telemetry and persisted
  maintenance lease/rate-limit state.
- Passive terminal output retention is off by default. When enabled in the VS
  Code extension, output is redacted and capped before it reaches the server.
- Databases and backups are local and unauthenticated by design. Any process with
  filesystem or stdio access can read or mutate them; do not place them in a
  shared or world-readable location.
