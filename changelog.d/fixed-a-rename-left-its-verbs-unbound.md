- **A rename left its verbs unbound, and unbound verbs let a caller name
  itself.** `requires_session`, the optional-session list and the heartbeat list
  are keyed by tool name, and the ADR-0059 task and design collapses moved the
  names out from under all three. Ten of twenty-three session bindings pointed
  at tools that no longer existed — `claim_task`, `complete_task`, `pause_task`,
  `resume_task`, `release_task`, `renew_lease`, `recover_claim`, `ask_question`,
  `pending_questions`, `register_design` — so `task_claim`, `task_transition`
  and `constitution_decide` bound no session at all. That is worse than
  unauthenticated: `apply_session_contract` strips `agent` only from a bound
  tool, so all three advertised `agent` for the caller to supply, and the
  server took it. Taking a claim, completing work and changing constitutional
  law could each be performed in another agent's name, which is the one thing
  resolving a session exists to prevent; the ledger's attribution was
  unenforced for the whole window. The heartbeat list broke the same way and
  more quietly: renewal-on-activity (ADR-0052) stopped firing for reading a
  task's scope and for asking or answering a question, so a lease could lapse
  while its owner was working and the next call told the rightful owner the task
  was not held by it. All three tables are now read against the call as it will
  actually be dispatched, so a rename carries its behaviour with it, and the two
  that had to distinguish acts within a collapsed cluster name the act rather
  than the tool. Two guards make the class un-repeatable: no advertised tool may
  declare `agent`, and every tool named by a server-side table must be one that
  is actually advertised — so the next rename fails a test instead of silently
  unbinding the verb.
