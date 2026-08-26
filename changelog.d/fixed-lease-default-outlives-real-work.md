- **Fixed:** the default lease for `task_claim` (`claim`/`renew`/`recover`/
  `resume`/`answer`) is now 1800 seconds (30 minutes), up from 300. The
  activity heartbeat added by ADR-0052 only renews on five specific
  Lodestar-side calls (reading a task's scope, asking/answering a parked
  question, checking conformance) — none of which cover the dominant shape
  of real work (editing files, running a build or test suite, committing),
  so a claim routinely lapsed mid-task with no heartbeat in sight. Measured
  at 27 lapses across 24 tasks and roughly 100 hours of work sitting under a
  dead lease, including an incident where a lapsed-but-finished task's
  already-published PR was duplicated by a rescuer reading the lapse as
  abandonment (`gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md`).
  See the [ADR-0052 amendment](docs/adr/0052-a-lease-is-a-heartbeat-not-a-deadline.md)
  for the revised trade-off: a genuinely wedged agent now takes longer to
  free, but `check_overlap`/`stalled` already give a human that path
  regardless of lease length.
