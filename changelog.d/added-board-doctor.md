- **`task_query(view="doctor")` diagnoses the board instead of leaving you to
  read every row.** It reports three conditions no other view surfaces:
  identical titles live under one goal, one title forked across several goals,
  and work `blocked` on no predecessor so nothing will ever unblock it. Each
  finding names the ailment, the task ids oldest-first, the title, and a
  suggested repair. It is read-only and judgement-free by design (ADR-0015):
  which of two identically titled tasks is the real work is a call only the
  reader can make, so the doctor never abandons, blocks or reopens anything.
  Terminal work is excluded, so acting on a finding clears it. Each ailment is
  grounded in a condition that had to be repaired by hand on this repository's
  own board — `stalled` reports lateness, and nothing about a duplicate or an
  ungated block is late.
