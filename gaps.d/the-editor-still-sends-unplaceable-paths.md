- **The extension cannot place files from unopened sibling worktrees — MEASURED,
  OPEN.** `editors/vscode/src/extension.ts` sends
  `vscode.workspace.asRelativePath(doc.uri, false)`, and that API returns its
  input *unchanged* when the file sits outside every workspace folder. Agents
  routinely edit a sibling worktree (`MindLeak-build`, `MindLeak-telemetry-poll`,
  `MindLeak-rustimports`) from a window rooted somewhere else, so an absolute
  path goes out on the wire and the server cannot place it either.

  Measured on the live graph at 2026-07-29T23:53Z: absolute artifact nodes were
  being created *seconds before the query* — `created_at` 1785369212 and
  1785369182 against a wall clock of 1785369218. This was never purely legacy
  pre-ADR-0038 data, which is why repair alone kept losing ground: 34 absolute
  nodes fell to 17 while other agents worked, and fresh ones arrived behind them.

  Ingest now refuses an unplaceable path, so the duplicate identity can no longer
  be created and `repair_workspace_paths` is no longer racing a live producer.
  What remains is the producer itself: the extension should resolve the path
  against the checkout that owns the file — or decline to ingest a file it cannot
  place — rather than handing the server a path only that worktree understands.
  Until then, saving a file from an unopened sibling worktree logs an ingest
  error instead of recording the file. That is the intended trade (a visible
  failure beats a silent second identity), but it is not the end state.
