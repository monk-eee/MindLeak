- **A guard that has to be told what to check stops covering the next thing
  added.** The check that catches a server-side table naming a tool that no
  longer exists was given the tables to inspect, so it protected exactly the
  ones somebody had remembered to register with it — and forgetting is the
  entire failure it exists to prevent. That was not hypothetical: a fourth
  table, `REQUIRED_SESSION_ACTS`, was added after the guard was written and
  covered only because its author happened to also edit the guard. The next one
  would have been invisible, in the same silence that let ten stale session
  bindings survive a rename. The guard now discovers every `ToolAct` table by
  reading the source, so a table is covered the moment it is declared rather
  than when someone thinks to mention it. Proven by declaring a new table that
  names a tool which does not exist and is referenced nowhere else: the guard
  fails and names it. Two details carry the honesty of the scan. Emptiness is
  asserted per table rather than in total, because a scan that quietly stopped
  reading one table still satisfies a total — that is how a scan in this file
  passed on its own source for weeks. And the string the scan searches for is
  built at run time, so this file does not contain the literal being looked
  for; a guard that searches for text matches itself and then reads its own
  body as data, which has happened here before and did again while writing
  this one.
