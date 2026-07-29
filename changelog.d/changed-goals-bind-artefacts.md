- **A goal may now bind the artefact it actually delivers, and the verb is named
  for it.** `link_goal_to_code` bound code, and a `governed` binding to a
  documentation node was discarded before it could be classified — so a goal
  whose delivery *is* an ADR, a doc, a benchmark or a build script had no way to
  say so. `touched_task_goal` was vacuously false for that work, and the finding
  "does not touch code bound to the task goal" was attached to tasks that had
  touched precisely the artefact their goal named; the only way to silence it was
  to bind an unrelated source file. The documentation exclusion now applies only
  to the *drift* branch, which is the case it was written for: an honest
  changelog touch still never drifts against a goal that merely bound it, but a
  doc bound to a task's own goal (or to a goal the task declared it covers)
  counts in scope. `link_goal_to_code` and `unlink_goal_from_code` are renamed to
  `link_goal_to_artifact` and `unlink_goal_from_artifact`, with every caller
  migrated in the same change and no alias shipped beside them (ADR-0059,
  ADR-0060).
