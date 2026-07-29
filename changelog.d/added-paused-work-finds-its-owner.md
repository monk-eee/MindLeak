- **Paused work now finds its owner or an accountable successor (ADR-0070).**
  `open_session`, `claim_task` and `renew_lease` return `paused_by_you` with the
  task, parked time and exact pause reason, plus the `resume_task` action; empty
  reminders are omitted. A paused task whose owner is known gone may now be
  transferred before the seven-day grace through the existing `recover_claim`
  path when a distinct human reviewer, expected owner and reason are supplied.
  The reviewer and reason are recorded in the task event/thread history and the
  successor starts a fresh evidence window. Agent-only takeover, `needs_input`
  recovery and the ordinary grace-based fallback are unchanged. The reviewer
  label is explicitly an attributable declaration, not authentication.
