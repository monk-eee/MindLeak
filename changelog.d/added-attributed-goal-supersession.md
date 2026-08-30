- Superseding a goal now records who retired it and when (ADR-0144). That makes
  the act attributable, and `ledger_act_evidence` accepts a new
  `goal_superseded` kind — so an agent whose task was to retire a clause can
  complete it with real evidence instead of routing to human review every time.
  Two places already claimed this was "attributed" while recording no actor at
  all; both are now true. **Breaking:** `supersede_goal` (and
  `constitution_define(action="supersede")`) now require `session_id`, rather
  than accepting an optional attribution that would be absent on exactly the
  calls that matter. A clause superseded before this change has no recorded
  actor and is refused by name, never attributed to whoever asks.
