- **The supervisor daemon's durable outbox now actually holds directive
  receipts, and its reconnect check no longer compares a number to its own
  echo.** Two defects shipped with the runnable daemon (ADR-0116 slice 5).
  First, `serve_once` opened the outbox and never enqueued anything, so a
  receipt computed by the local inbox and then lost to a dropped connection
  depended entirely on the server redelivering its directive — a guarantee held
  by the other side of the connection that had just failed. Receipts are now
  queued durably before transmission and acknowledged only once Ackplane's own
  frame receipt confirms them, and anything still owed is resent on the next
  connection.
  Second, the reconnect reconciliation could not work: the daemon sent its own
  position as `last_accepted_position` and the server echoes that same value
  back as `HelloAccepted.accepted_position`, so the comparison was a tautology
  that could only ever answer "up to date". That call is removed rather than
  left reading as a working guard. Detecting a server that holds more
  supervisor evidence than the node can account for needs the server to report
  its own independent view, which the wire protocol does not carry; that is
  recorded as an open gap rather than half-built.
  A frame Ackplane refuses as non-retryable is now dropped from the queue with
  a loud error instead of being resent on every reconnect, so one unacceptable
  receipt cannot wedge the daemon in a retry loop and block every later frame
  behind it.
