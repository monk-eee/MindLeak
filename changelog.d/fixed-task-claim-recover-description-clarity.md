- **`task_claim`'s tool description no longer reads as if `recover` is the
  general way to take over a stranded claim.** A plain `claim` already
  succeeds for any agent once a task is `open` or `claimed` with an expired
  lease (unchanged behavior, matching the existing
  `expired_lease_is_reclaimable_by_another_agent` test) — `recover` exists
  only for a pre-ADR-0054 legacy owner string or an early `paused`-task
  transfer with a human reviewer. The tool description and the
  `recover`-refusal error message now say so directly, so an agent hitting
  "claim recovery requires a compatible legacy owner" is pointed at `claim`
  instead of retrying `recover` or inventing a workaround. No behavior change.
