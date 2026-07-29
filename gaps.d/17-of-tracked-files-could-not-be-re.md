- **17% of tracked files could not be re-ingested at all: another worktree's
  absolute id owned them — MEASURED, FIXED.** Found by the first run of
  `make reingest`. 43 of 247 files failed with

  ```
  structural edge is owned by
  artifact:c:/Users/lyndonswan/Repos/MindLeak-export/scripts/design-audit.mjs,
  not artifact:scripts/design-audit.mjs
  ```

  ADR-0038 gives every worktree of this repository one shared graph, so an
  absolute id written from `MindLeak-export` names the *same file* as the
  repo-relative id — but it held the structural edges, and the repo-relative id
  could not take them. Those files were frozen under whichever worktree ingested
  them first, so every future extractor improvement missed them and nothing
  reported it; the error only appeared if something tried to re-ingest.
  Repair was prefix-scoped, which assumes every worktree eventually hosts a
  server that heals its own ids — untrue for a worktree an agent works in
  without ever starting one there. An absolute id now merges onto a
  repo-relative twin **the graph already holds**, taken as the longest matching
  suffix; a path with no twin is still left alone, so repair never invents a
  file and `repair_is_idempotent_and_leaves_foreign_paths_alone` stands
  unchanged.
  Two further defects only surfaced once that was fixed and the files stayed
  blocked, and they are the more useful finding. `edges.owner_id` records which
  artifact owns a structural snapshot (ADR-0007), and merging a node rewrote the
  edge *endpoints* but never the *ownership* — a hole that predated all of this
  and affected same-root repairs too. Ownership is not an endpoint, so it
  survives the node it names being deleted, and with the absolute node already
  collapsed there is nothing left for a node-level repair to find: the file
  becomes permanently un-re-extractable, quietly. And the facade repair was a
  no-op without a declared workspace root, which is exactly the case that
  stranded sibling ids in the first place. Ownership is now carried across a
  merge and reclaimed by a pass keyed off ownership rather than nodes, and the
  collapse runs with or without a declared root.
