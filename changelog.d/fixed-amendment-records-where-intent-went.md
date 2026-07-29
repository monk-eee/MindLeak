- An amendment now records where each clause went and carries the work with it.
  Superseding a clause used to leave `superseded_by` NULL, and because an
  amendment renames every clause it carries forward (`goal:{slug}@{version}`),
  nothing could follow the rename: code bindings and open tasks kept naming a
  clause no active constitution contained. `amend_constitution` now names the
  successor by slug and moves goal/code bindings and non-terminal tasks onto it
  in the same transaction. Terminal tasks keep their original clause, because a
  finished audit must keep naming what it was judged under.
- A migration reconnects clauses already stranded this way. On this repository
  that moved 156 bindings and 53 tasks onto the active constitution and recorded
  26 successors, leaving all 178 finished tasks untouched. A task held under an
  unexpired lease is skipped: its goal is what conformance judges the holder's
  evidence against, so moving it mid-claim would change the rule beneath someone
  doing the work (ADR-0063). Those move at the next amendment instead. Recorded
  in ADR-0068.
