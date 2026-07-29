- **`make stranded-report` turns a lapsed claim into a judgement rather than an
  investigation, and `board-health` stops implying an agent could close one.**
  A lapsed claim cannot be closed by an agent, and the reason is structural
  rather than a gap: closing one means re-claiming it, and re-claiming after a
  lapse records the lapse, whereupon conformance returns `needs_human` for a
  discontinuous evidence window and refuses to certify across the hole.
  Narrowing the window around the gap is exactly the laundering ADR-0048 exists
  to stop, so the refusal is the guarantee working. Measured while trying: a
  task showing `0 lapse(s)` reported `the lease lapsed 1 time(s), leaving
  85730s unleased` the moment it was claimed in order to close it. Calling them
  "stranded claims" invited precisely the response that cannot work, so the
  report now says `awaiting confirmation` and names who can act. The new report
  proposes the commit that most likely shipped each one, graded strong / likely
  / weak / none — a close second downgrades the confidence, because a coin toss
  presented as a finding turns a judgement into a rubber stamp.
