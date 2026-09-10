- Stop remaining worker-owned descendants before reporting completion or releasing
  the process-group handle; repeated terminal observations do not signal the
  finished group again. A bounded socket regression reproduces the exited-parent
  case that previously left a descendant running.
- Reconcile receipts against the outbox's full retained interval, so shutdown or
  reconnect can safely replay server-accepted frames whose acknowledgements were
  lost. Refuse genuine evidence loss beyond either retained boundary. This fixes
  the shutdown race reported by the industrial and coverage CI jobs.
