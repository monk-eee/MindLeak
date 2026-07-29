- **A sibling worktree is not a foreign path, and structural ownership now
  follows a merged identity.** Two defects, one symptom: 43 of 247 tracked files
  could not be re-ingested at all, so every future extractor improvement would
  have missed them silently.
  Repair was prefix-scoped, which assumes every worktree eventually hosts a
  server that heals its own ids. A worktree an agent works in without ever
  starting a server there leaves its ids orphaned permanently. Since ADR-0038
  gives every worktree of a repository one shared graph, an absolute id written
  from a sibling checkout names the *same file* as the repo-relative id — so it
  is now merged into it, whichever checkout spelled it.
  The warrant is evidence, not a guess about where a checkout begins: the merge
  target must be a repo-relative id **the graph already holds**, taken as the
  longest matching suffix so a full path always beats a bare filename that
  happens to collide. A path with no such twin is still left exactly alone, so
  repair never invents a file — the rule the prefix pass was protecting, and the
  existing `repair_is_idempotent_and_leaves_foreign_paths_alone` test, both
  stand unchanged.
  The second defect only surfaced when the first was fixed and the files stayed
  blocked. `edges.owner_id` records which artifact owns a structural snapshot
  (ADR-0007), and merging a node rewrote the edge *endpoints* but never the
  *ownership* — a hole that predates this change and affected same-root repairs
  too. Ownership is not an endpoint, so it survives the node it names being
  deleted, and `replace_structure` then refuses every later ingest of that file
  with "structural edge is owned by <absolute id>, not <relative id>". With the
  absolute node already collapsed there is nothing left for a node-level repair
  to find, and the file is permanently un-re-extractable. Ownership is now
  carried across a merge, and reclaimed separately by scanning ownership rather
  than nodes, so the already-orphaned state heals too.
  Repair also no longer needs a declared workspace root to do this. The prefix
  pass still does and is still skipped without one; collapsing ids whose twin
  the graph already holds does not, and a server that never declares a workspace
  was exactly the case that left sibling ids stranded, because no root ever
  matched them.
  Verified against the live graph: all five sampled blocked files now ingest.
