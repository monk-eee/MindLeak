- **Duplicate node identities are collapsed, and the survivor keeps the history
  both halves earned.** Making paths repo-relative stopped new splits; it did
  nothing about the 590 files already living under two identities. A repair pass
  now rewrites node ids that spell their path absolutely under the served
  checkout onto the repo-relative id, **merging** rather than choosing a winner:
  reinforcement counts add, weight carries the write path's own `+0.05` per
  reinforcement, the earliest `first_seen` and latest `updated_at` survive, and
  the longer-lived half-life follows the more recent edge. Picking a winner
  would have been the expedient choice and would have silently discarded real
  corroboration — the thing signal-weighted decay (ADR-0005) exists to reward.
  Measured on this repository: **871 absolute ids across 8 worktrees collapsed to
  0, and 590 duplicated files to 0**, taking the graph from 6,144 nodes to 5,106
  without losing an edge's history. The pass runs at startup, is idempotent, and
  is scoped to the checkout the process serves, so each worktree heals its own
  ids and the graph keeps healing if any producer ever regresses — one worktree
  running an older binary during the migration was found and healed exactly this
  way. A repair failure logs and never blocks startup: a graph with split ids is
  still usable, and refusing to start would be the larger outage.
