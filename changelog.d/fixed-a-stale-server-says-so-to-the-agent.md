- **A stale server now says so to the agent using it, not only to a log the
  agent cannot see.** `build_notice` already decided this correctly and
  `BuildNotice` already carried a `stale` flag — but the answer went to stderr
  at startup, and an MCP client shows that to nobody. The comment above the
  check had already recorded the same failure one level up: the version *"has
  always been reported at `initialize`; nobody compared it, and a two-day-old
  local build cost a night of misdirected debugging"*. Moving it to stderr fixed
  where it was written, not whether it was read.
  Measured over a full session on 2026-07-29: every call ran against a binary
  built from `f9a549c4211a` while the checkout had moved on, and nothing in the
  tool surface said so. The consequences were diagnosed as tool defects rather
  than as a stale build — `propose_amendment` and `amend_constitution` were
  "missing from the tool list" because the binary predated them, and the sha
  validation that refuses a fabricated commit id was not running at all while
  every call still looked normal.
  `open_session` now carries the notice, on both planes, because it is the one
  call every agent already makes before anything else — the same reasoning that
  put commit provenance on the commit rather than on remembering to record it.
  Only when the binary is genuinely behind the checkout it serves: a current
  build says nothing, so the field keeps its meaning instead of becoming a line
  to scroll past. It reports and never refuses, because a server that stopped
  serving because it was behind would halt a fleet mid-flight, which is worse
  than the staleness it is complaining about.
