- `merge_evidence` builds a conformance evidence bundle from a merge that
  already landed (ADR-0058), instead of the agent assembling one by hand.
  Name the commit that carried the work; the plane verifies deterministically
  that git can resolve it, that it is reachable from `main`, and that it touched
  paths inside the task's declared scope, then derives the bundle from what git
  reports. It refuses a commit that never merged, one outside the task's scope,
  one the calling agent does not hold the task for, and a task that declared no
  scope at all — with nothing to match on, any merged commit in the repository
  would otherwise serve as a receipt.
  It does not complete the task: conformance still judges the result and
  somebody still has to submit it.
