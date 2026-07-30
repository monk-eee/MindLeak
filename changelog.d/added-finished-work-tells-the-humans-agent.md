- **Work that finished and is waiting on a person now says so where the human's
  agent already looks.** A `drift` or `needs_human` verdict completes a task
  into `in_review` rather than `done` — the honest outcome, and by design
  (ADR-0009). Only a person can finish it. But nothing told anyone: completing
  into `in_review` clears the owner, and a human has no agent id (ADR-0046), so
  there was no agent to notify and no queue to read. Measured on 2026-07-30,
  five tasks were sitting finished from at least three sessions, three of them
  more than a day old, surfaced nowhere but a board query somebody had to think
  to run.
  `open_session` now carries `awaiting_a_human` alongside the `stale_build`,
  `waiting_on_you` and `paused_by_you` it already reports. The agent is told
  because the agent is the only thing the human talks to.
  It is a **filter over `stalled_work`'s existing `awaiting_human` rule**, not a
  second query. Deriving "waiting on a person" twice would let the two surfaces
  disagree about what that means, and the one that drifted would be the one
  nobody tested. The query lives on the facade rather than in the response, so
  the fact is available to any caller.
  Read-only and advisory: it reports and can never refuse. It says **nothing**
  when the queue is empty, because a field that always appears is one readers
  learn to scroll past — the same reason `stale_build` stays quiet on a current
  build. Both the reporting and the silence are tested.
