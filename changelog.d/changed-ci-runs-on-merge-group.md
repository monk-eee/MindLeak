- **CI now triggers on `merge_group`, the prerequisite for a merge queue
  (ADR-0061).** Enabling a queue without it would have deadlocked delivery
  completely: the queue runs the required checks against a temporary
  `merge_group` ref holding the prospective merged result, and a required check
  that does not trigger on that event never reports — so the queue waits for it
  forever and nothing merges at all. All five required checks come from
  `ci.yml`, which triggered only on `push` to `main` and on `pull_request`. The
  trigger is inert until a queue exists, which is exactly why it lands first and
  on its own.
