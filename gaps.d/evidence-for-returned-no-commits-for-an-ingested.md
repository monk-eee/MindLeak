- **`evidence_for` returned no commits for an ingested, attributed, in-window
  commit — OBSERVED ONCE, NOT REPRODUCIBLE. Treat as a false alarm until seen
  again.** — Recorded 2026-07-28 as an open defect, then disproved the same day.
  It is kept rather than deleted because the original entry was wrong in a
  dangerous direction, and the disproof is the useful artefact.

  What was seen once, closing `task:88a9c02d9c5d`: `ingest_commit` reported
  `nodes_created=2, edges_created=3`; `intent:e0585eb` existed with
  `type=intent` and `created_at` inside the window; `working_set` for the same
  agent returned that commit; `evidence_for` returned `commits=0`.

  What was then tried, all of which **pass**:

  | probe | result |
  |---|---|
  | red integration test — ingest, attribute, bound, assert (`evidence_for_returns_a_commit_the_agent_ingested_inside_the_window`) | returns the commit |
  | live graph, 5-second window | returns the commit |
  | live graph, 6-minute / 2-hour / 24-hour windows | returns the commit; count grows 1 → 2 → 3 → 4 |
  | live graph, with and without `task_id` | identical |

  So the query is not window-sensitive, not truncated by the event `LIMIT`, and
  not affected by `task_id`. The single failure was not reproduced and no cause
  was isolated.

  **The reason this matters more than an ordinary false alarm:** conformance
  answers `needs_human` with *"evidence contains no provenance-bearing
  mutation"*, and a broken evidence query and an honest refusal are
  indistinguishable from outside — both say `needs_human`. That symmetry is what
  made a one-off look like a systemic defect, and it is why the original entry
  overstated it. Five tasks were closed on that verdict on 2026-07-27/28 and, on
  the evidence above, those verdicts were the contract working correctly against
  lapsed leases and forked identities.

  **Still do not "fix" this by widening the evidence window.** That was the wrong
  fix when it looked like a bug and it is a worse one now that it does not.
