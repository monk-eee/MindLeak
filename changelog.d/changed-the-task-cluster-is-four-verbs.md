- **The twenty-six-tool task cluster is now four verbs.** Creating work,
  owning it, moving it through the lifecycle and reading the board each had
  several tool names — twenty-six entries for four things an agent does, in the
  cluster every session touches. They are now `task_create`, `task_claim`,
  `task_transition` and `task_query`, with the act named as an argument
  (`step`, `to`, `view`), following the same rule ADR-0059 applied to the design
  cluster: where a cluster moves one entity through a state machine, the tool
  surface should reflect the machine rather than enumerate it. Every guard is
  now an argument validation carrying the same message, and each refusal names
  the transition that wanted the argument, which the old flat `missing required
  string arg: reason` could not. The twenty-six old names answer for one minor
  version and each reply names the call to make instead; removal ships with the
  release train ADR-0059 names.

  Two guidance strings changed with them, because they name a call an agent is
  expected to make next: a lost claim now says `task_claim with step="recover"`
  can take it over, and an expiring lease says to call `task_claim with
  step="renew"`. Advice that names a verb nobody will find is worse than no
  advice.

- **A deprecated tool name silently lost its argument checking.** Argument
  validation looks a tool up by name to find the schema to check against, and a
  collapsed cluster's old names are deliberately absent from that list — so for
  every caller still using an old name, `validate_arguments` found no schema and
  returned "fine". That is precisely backwards: the callers on the old names are
  the ones most likely to get an argument wrong, and the window lasts a whole
  minor version. The incident this guard exists to prevent — `lease_seconds`
  passed where the tool declares `lease_secs`, silently dropped, the default
  applied, and the claim lapsed mid-work — was reachable again through the old
  name. Deprecated names are now validated against the schema that actually
  answers them, which is also the schema whose argument list the error message
  should be quoting. This shipped broken with the design cluster in the previous
  release and is fixed for both.

- **The three collapsed clusters now share one deprecation implementation.** The
  rename table, its "call this instead" notice, and the two argument helpers
  every collapsed tool needs (`one_of`, and the conditional-requirement message
  that names which transition wanted the argument) were written for the design
  cluster and about to be copied a third time. They live in `tools/mod.rs` now,
  with each cluster owning only its own table of names — which is also what lets
  argument validation find every rename from one place instead of asking each
  module in turn.
