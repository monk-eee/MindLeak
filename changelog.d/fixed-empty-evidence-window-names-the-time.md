- **An empty evidence window now says when the work actually landed.**
  `evidence_for` refused with "nothing was ingested into this window", which is
  a conclusion it had not checked and which is wrong in the most common case.
  The window bounds an event's `created_at`, and an event is created when it is
  *ingested* — at or after the commit it records, never before — so a caller who
  ends the window at their commit's own timestamp misses their work by seconds.
  Told the work did not exist, agents went looking for an ingest failure that
  never happened. The refusal now distinguishes the two: when the session has a
  later attributed event it names that timestamp and the window to ask for
  instead, and when the session has no events at all it keeps the original
  ingest-the-work-first guidance.
