- **Prior work is found across a constitution amendment.**
  `existing_work` matched goals with an exact `goal_id` compare, but an
  amendment re-issues every clause as `goal:<slug>@constitution:vN` while tasks
  go on naming whichever form they were created under. A retry created under
  the bare slug therefore could not see work already finished under the
  versioned id, and `task_create` answered `already_serving_this_goal: 0` for
  work that plainly existed — exactly when that question is being asked in
  order not to repeat it. Measured on the live board: 11 titles had been
  created more than once across 29 tasks, and 5 of those spread their attempts
  across ids sharing a single slug. The worst, "Carry controls across an
  amendment", was created six times: the attempt that finished sat under
  `@constitution:v2`, and all five abandoned retries under the bare slug, every
  one of them blind to the completed work. Goal matching now reuses
  `goal_slug` — the same rule `store::goals` and the clause binding already
  use — so the versioned and bare forms find each other.
