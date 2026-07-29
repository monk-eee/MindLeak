- **Advertised task verbs keep their session contract.** The task-cluster
  collapse renamed twenty-six tools to `task_claim`, `task_transition` and
  `task_query`, but the server policies that resolve session identity and renew
  an active lease still recognized only the deprecated names. The replacement
  calls therefore reached dispatch without their registered agent:
  `task_claim` could not claim anything, session-bearing transitions such as
  `complete` were refused, and overlap queries lost their branch context. The
  server now canonicalizes deprecated calls before applying one
  operation-aware policy. Every ownership step is session-bound; only the
  transition and query variants that need identity require it; overlap remains
  an anonymous advisory when no resolvable session is offered; and heartbeat
  behavior is identical through either vocabulary during the deprecation
  window. The advertised schemas expose `session_id` without accepting a
  caller-asserted `agent`.
