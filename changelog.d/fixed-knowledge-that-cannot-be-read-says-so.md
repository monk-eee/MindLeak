- **Knowledge that can never be read now says so when it is written.** The
  conformance advisory matches recorded knowledge on referenced nodes and
  nothing else, so a record whose evidence carries no `nodes` array is stored,
  counted, and permanently unreachable — it can never arrive in front of the
  agent it was written for. Nothing reported that at the point it happened.
  `active_knowledge` already exposes a `surfaces` field, but reading it requires
  already suspecting the problem, and an agent recording a lesson for a
  colleague has no reason to suspect anything: the call succeeds, returns an id,
  and looks exactly like one that worked. Measured before this landed, 3 of 17
  active records were invisible, among them one recording the cost of skipping
  the mandatory `advise` pre-flight — written precisely so the next agent would
  not repeat that mistake, and structurally incapable of reaching them.
  `record_knowledge` now reports `surfaces` in its own response, and when it is
  false it says which field is missing and what to put in it. Write time is the
  only moment this is worth saying, because it is the only moment the caller
  still has the node ids to hand; afterwards the information needed to fix the
  record is gone along with the context that produced it. The record is kept
  either way and is not refused, since losing an agent's stated lesson to a
  formatting mistake is worse than storing one that cannot yet be matched.
