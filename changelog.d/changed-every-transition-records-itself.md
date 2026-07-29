- **Every task transition now records itself in the log.** ADR-0064 step two:
  the sixteen verbs that mutate task state — claim, renew, heartbeat, block,
  reopen, abandon, resolve, ask, answer, pause, resume, release, both
  conformance transitions, claim recovery, and creation itself — append a typed
  event inside the same transaction as the guarded write they perform. Four
  verbs that previously wrote outside a transaction (`renew_lease`,
  `touch_lease`, `resume_task`, `release_task`) now open one, because a record
  that can commit separately from the row it describes is not a record.
  The claim compare-and-swap is untouched. It is a single guarded UPDATE inside
  an Immediate transaction and it was already right; the event is appended
  beside it rather than atomicity being rebuilt on top of a log.
  `open_blocked_successor_on` resolves the successor before updating it instead
  of updating through a subquery, so a predecessor-driven unblocking can be
  recorded against the task it actually moved. That event has no actor by
  design: nobody asked for it, the gate simply lifted, and naming a caller would
  attribute a decision no agent made.
  `project_tasks` replays the log into task state, and a test walks a task
  through most of its lifecycle and asserts the replay reproduces the live board
  exactly. That test is the point: `tasks` is written through rather than
  rebuilt (ADR-0063 forbids a migration from touching a live claim), so "the
  projection is derivable from the log" is a property that would otherwise
  quietly stop holding the first time a verb forgot to record itself.
