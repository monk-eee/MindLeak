- **`done` does not mean `aligned`, but shipped work no longer remains silently
  claimable — NARROWED, MEASURED 2026-08-01.** The earlier fragment combined
  two different failures: work already merged still appearing open/free, and a
  completed task carrying a non-aligned receipt. The first is closed. Before
  this audit the live board held exactly two claimed tasks, both attached to
  current workstreams, with zero open, blocked, paused, needs-input, or
  in-review residue. Completion offers at publication, merge-derived evidence,
  `existing_work`, reviewed rescue, and explicit human resolution removed the
  ten known shipped-but-claimable examples recorded in the old measurement.

  The receipt distinction remains and should stay visible in reporting. Taking
  each of 330 `done` tasks' `resolved_conformance_id` when present and otherwise
  its latest conformance check yields 157 `aligned` (47.6%), 133 `needs_human`
  (40.3%), and 40 `drift` (12.1%). This improves on the prior 119 of 247
  `needs_human` result (48.2%), but it is not automated affirmation. A task can
  be `done` because a named person reviewed and resolved a non-aligned receipt;
  that is an auditable decision, not a claim that conformance aligned.

  Impact: delivery dashboards must report automated alignment separately from
  human resolution. Do not loosen evidence windows, erase lease lapses, or
  reinterpret a human resolution as `aligned` to improve the ratio. The
  remaining work is measurement and workflow quality: advise before task
  creation, claim before the first commit, keep the lease live, submit the
  publication offer promptly, and show both completion route and verdict in
  aggregate product metrics.

  NARROWED FURTHER, 2026-08-19: the aligned/needs_human/drift split above
  required a one-time manual audit query. `lodestar_stats` now reports it
  directly as `done_verdicts` (aligned/needs_human/drift/violation/unresolved),
  computed the same way -- human `resolved_conformance_id` when present,
  otherwise the task's latest conformance check, otherwise `unresolved`. Any
  caller can watch the ratio move without re-deriving the query each time. This
  closes the measurement gap, not the underlying fact: `done` still means
  shipped, not that conformance affirmed it, and a human can still resolve a
  non-aligned receipt for a good reason. Dashboards and agents should read
  `done_verdicts` rather than treating `done_tasks` as a proxy for correctness.

  RE-MEASURED 2026-08-26 via `lodestar_stats` directly (no manual audit
  needed, confirming the 2026-08-19 tooling fix still holds): of 803 `done`
  tasks, 485 `aligned` (60.4%), 240 `needs_human` (29.9%), 78 `drift` (9.7%) —
  up from 47.6% `aligned` on 2026-08-01 across roughly 2.4x as many done
  tasks. The ratio has moved in the direction this fragment asks dashboards to
  watch for, not away from it. This is not automated affirmation and does not
  change the underlying fact the fragment records: a human resolving a
  non-aligned receipt is still an auditable decision, never a claim that
  conformance aligned, and no code change accompanies this entry — it is a
  data point, recorded so the next reader does not have to re-run the query
  to know whether the trend is improving or eroding.
