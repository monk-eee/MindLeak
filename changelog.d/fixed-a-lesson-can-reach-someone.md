- **Every lesson an agent recorded on completing a task was invisible, and the
  tool that could have shown you was never advertised.** `complete_task` stored
  its `learned` note with the bare string `task:{id}` as provenance. That is not
  JSON, so it parsed to no referenced nodes — and referenced nodes are the only
  thing `apply_knowledge_advisory` matches on. Every lesson was written,
  counted in `lodestar_stats`, and delivered to nobody. Measured on this
  repository the moment it became measurable: **34 of 35 active knowledge
  records referenced nothing.** The note now carries the nodes the work changed,
  so a lesson learned while changing a file reaches the next agent who changes
  that file — the moment it is useful, and the moment it was written for.
- **`active_knowledge` is now advertised, and reports whether each record can
  ever surface.** The tool dispatched all along and appeared in no `definitions()`
  list, so from the tool surface the knowledge base looked write-only: record,
  promote, reconfirm, prune, and no way to see what was already known. It now
  takes an optional `node` (what is known about the thing you are about to
  change) or `contains` filter, and every entry reports `surfaces`, because a
  record that names no nodes is stored and silent and that should not have to be
  inferred from an empty array.
- **A tool the server answers to must be a tool it advertises.** A guard walks
  every dispatch block and fails on any name absent from the advertised list.
  This is the mirror of the undeclared-argument guard: there the contract asked
  for something it never mentioned, here the server answered to something it
  never mentioned, and both fail the same quiet way — the code is right, the
  advertisement is wrong, and nothing breaks loudly enough to be found.
