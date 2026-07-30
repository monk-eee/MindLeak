- **Five tasks are finished and waiting on a human, and nothing tells the
  human — MEASURED, OPEN.** A conformance verdict of `drift` or `needs_human`
  completes a task into `in_review` rather than `done`, which is the honest
  outcome and by design (ADR-0009). Only a person can finish it, with
  `task_transition to:"resolve"` under a reviewer label that must differ from
  the agent under review (ADR-0071) — correctly, since an agent resolving its
  own review would make the whole verdict ceremonial.

  The gap is that nothing surfaces the queue. `attach_owner_attention` puts a
  `waiting_on_you` array on `open_session`, but it carries unanswered
  *questions* only; work sitting in review appears nowhere except a deliberate
  `task_query view:"board"` that somebody has to think to run. Measured on
  2026-07-30: five tasks in `in_review`, from at least three different sessions
  — `task:c83a6ad5b2eb`, `task:4a5ab8ef345c`, `task:718e43f33aa4`,
  `task:543314cd6fa8`, `task:bbbe16677560`. Three of those had been sitting
  since the previous day.

  This is the same shape as the stale-build notice before PR #189: the answer
  was written where nobody reads. `open_session` is the one call every agent
  and every restart unavoidably passes through, and it already carries
  `paused_by_you` and `waiting_on_you`. A `waiting_on_review` array beside them
  would cost nothing and close it. Until then the review queue grows silently,
  and the failure mode is not a wrong verdict but an accurate one that no one
  ever acts on.
