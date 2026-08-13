- **`task_query(view="rework")` measures the rework rate ADR-0057 named and
  nothing could re-run.** The ADR recorded a baseline and said that if the rate
  does not fall, the coordination mechanism is wrong and should be removed
  rather than tuned indefinitely — a test no query could take, because the
  number was never returned. Over a window it now reports how many tasks
  repeated a title that already existed, how many of those were created in the
  same second as the task they repeat, and the worst repeated titles with the
  number of goals each was forked across. The same-second count is the one that
  says whether an advisory notice could have helped: a person or an agent
  deciding whether to start cannot produce two tasks in one second, so that
  share is machine fan-out, and a notice needs a reader. Unlike `doctor` it
  reads terminal work, because a duplicate that was retired still cost the
  seeding, the reading and the retiring. Abandonment is reported beside the rate
  and deliberately not counted as rework: work dropped once it turned out to be
  unnecessary is good judgement, and counting it as waste would flatter a fleet
  that never reconsiders anything. Redundancy is judged against the whole
  history, so narrowing the window never makes a repeat of older work vanish.
