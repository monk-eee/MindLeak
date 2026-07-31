- **A server still running a file that has since been replaced now says so.**
  `stale_build` answers "was this binary made from something old" by comparing a
  build sha against a checkout. It structurally cannot see the other fault: the
  binary was correct when the process started and the file underneath it was
  then swapped. A running process keeps reporting the sha it was compiled with,
  so it can match HEAD exactly while the code actually answering has been
  superseded on disk.

  Measured cost of that silence, 2026-07-30: most of a session. A live server
  kept answering with a pre-ADR-0054 labelled agent id while every binary on
  disk answered with the collapsed one, so the owner string flipped between two
  consecutive board reads with no intervening claim and the whole closing loop
  became unreachable — `check_conformance` refused with "evidence agent does not
  own the task". The first diagnosis was a stale deployment, and it was wrong:
  driving the same session token through every binary on disk returned the
  current answer. Rebuilding and reinstalling would have changed nothing.
  Restarting was the entire remedy.

  `open_session` now carries a `replaced_binary` notice on both planes, asked
  fresh on every call because the swap happens after startup and a value
  computed once could never see it. The advice is deliberately to **restart, not
  rebuild** — rebuilding is the instruction that wasted the session. An
  executable whose timestamp cannot be read stays silent rather than
  manufacturing a warning, the same distinction the surrounding module already
  draws between an unanswerable question and an answer of no.

  Detection is not prevention: the agent is told, and nothing restarts for it.
