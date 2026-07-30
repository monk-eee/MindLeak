- **A newly started agent now receives work whose owner disappeared.**
  `open_session` conditionally returns `rescue_work` for expired claims and
  deadlocked wait cycles already identified by Lodestar's durable
  `stalled_work` projection. Each entry names the prior owner and branch when
  known, explains the stall, and includes the canonical `task_query` action to
  inspect it plus the `task_claim` action that can take an expired claim.
  The field is absent when there is nothing to rescue. It is read-only: opening
  a session never steals, closes, or otherwise mutates work. Ordinary
  peer-addressed questions remain in `waiting_on_you`, deliberate pauses remain
  with their owner, and completed work awaiting a person remains in
  `awaiting_a_human`, so the rescue signal does not become another noisy board.
