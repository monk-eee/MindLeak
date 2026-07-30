- The silent-knowledge audit counted one of the two ways a lesson reaches an
  agent, so it reported records as dead that were arriving: it called 68 of 210
  unreachable, where 12 are. Reachability now has a single definition —
  `Lodestar::knowledge_reach` — and `record_knowledge`, `active_knowledge` and
  `scripts/silent-knowledge.mjs` all ask it rather than each deciding for
  themselves, which is how three readers of one rule came to be falsified
  together by a single commit. The report now separates records reaching agents
  by the nodes they name from those reaching only the goal they were learned
  under, and says how many of the latter are crowded out by that path's
  per-check cap.
