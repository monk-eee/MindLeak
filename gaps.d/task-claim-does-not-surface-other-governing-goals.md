- **`task_claim` does not surface goals that govern the declared paths but differ
  from the task's goal — RE-MEASURED 2026-07-31, OPEN, low severity.** The claim
  response builds `governing` from `code_for_goal(task.goal_id)`, so it can list
  only artifacts bound to the goal already chosen for the task. It cannot say
  that a declared path is governed by another goal, which is the set that would
  produce drift. A quiet claim therefore still looks like "nothing else governs
  these paths." The current contract now lets `task_claim` union additional
  `also_serves` goals, so the older claim that discovery at claim time is too
  late is no longer true: a warning there would be actionable. The warning is
  simply not computed. Impact is limited because ADR-0029 already requires
  `advise` before claiming; this is the missing backup diagnostic when that
  pre-flight is skipped. Not fixed in this run; split from the unbound-new-file
  gap when ADR-0078 mechanised that separate condition.
