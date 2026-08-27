- **An Applied supervisor directive could reopen terminal Industrial Work tasks
  -- VERIFIED 2026-08-27, repair in progress.** `apply_task_effect` locked the
  task but dispatched Assign, Drain, Pause, and Resume without first rejecting
  completed or abandoned states. A delayed Drain receipt could therefore clear
  terminal ownership/lease data, reset the projection to Open, increment its
  version, and append a state-transition event even though the task had already
  reached a terminal outcome through another valid path.
