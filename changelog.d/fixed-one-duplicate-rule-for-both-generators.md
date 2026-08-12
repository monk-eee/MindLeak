- **The two task generators now share one answer to "has this already been
  produced?"** Decomposition compared goals by slug and design materialization
  compared the goal id exactly, so an amendment — which re-issues a clause as
  `goal:<slug>@constitution:vN` while tasks keep naming the form they were created
  under — would have made materialization report every pre-amendment task absent
  and rebuild all of it, the duplicate board this rule exists to prevent arriving
  the first time the constitution is amended. Both now use one slug-matched
  lookup, and it is asked per draft rather than from a snapshot, so a model that
  emits one title twice in a batch is caught by the same rule.
