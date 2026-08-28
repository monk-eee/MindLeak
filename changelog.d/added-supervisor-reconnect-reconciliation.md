- **A reconnecting supervisor now reconciles its position honestly, and says so
  when it cannot (ADR-0116 slice 4, decision 7).** Slice 3 proved a clean
  redelivery survives a reconnect; what was missing was the supervisor deciding
  *whether* the resume is clean. `SupervisorOutbox::positions` reports what the
  durable outbox can actually prove — derived from the surviving frames rather
  than a second stored counter that could disagree with them — and `reconcile`
  turns that plus the server's accepted position into one of three answers.
  The asymmetry is the point. A server holding **less** than the supervisor is
  ordinary: frames were queued and not yet accepted, so the supervisor resends
  from the server's position. A server holding **more** cannot be fixed by
  resending: the supervisor's own durable record is behind reality, so there are
  frames it published and can no longer describe. That is reported as
  `IncompleteEvidence` and blocks resumption, rather than being papered over as
  a clean resume — a wiped outbox reports position zero, which is also a
  perfectly legitimate value for a supervisor that has never run, so only the
  comparison against the server tells "new" apart from "lost".
  A directive whose window closed while the supervisor was disconnected is
  receipted `expired` on redelivery and closed out, rather than dropped or
  redelivered forever.
