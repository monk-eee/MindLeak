- **A stalled wait is only bounded by the seven-day parking grace — SURFACED,
  not prevented.** — ADR-0046 lets `ask_question` address a peer, so an agent
  can park on one that never answers. The mutual case (a wait cycle) is now
  detected and reported by `fleet_view`, and answering any named task breaks it.
  What is *not* solved: a one-way wait on an agent that has vanished is not a
  cycle and is not flagged — correctly, since the addressee could still answer,
  but it means a task can sit parked for a week on someone who is never coming
  back. Nothing alerts either way: `fleet_view` is a pull, so the finding is only
  seen if a human or agent looks. Impact: bounded wasted wall-clock, never
  permanent. Fix would be a staleness threshold on an unanswered wait — an
  addressee with no live claim and no recent session is a different, weaker
  signal than a cycle and should read as such.
