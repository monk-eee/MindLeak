### Fixed

- **A covering task is recognised again after a constitution amendment.** Every
  task touching governed code completed as `drift`, however correct it was, and
  `claim_task` / `governing_for_task` reported that nothing governed the change
  — so an agent was waved through on governed code and only found out at
  completion.

  A clause carried into a new constitution is re-issued as
  `goal:<slug>@constitution:vN`, while a task keeps naming the bare
  `goal:<slug>`. Coverage was decided by string equality on those two ids, and
  binding lookup by exact goal id, so from the *first amendment onwards* neither
  could ever match. Both now compare by slug — the identity a clause keeps
  across versions, which the amendment carry-forward and `diff_clauses` already
  use.

  The empty binding list was the worse half: it is indistinguishable from "this
  goal governs no code", so the failure reported itself as a clean bill of
  health. A verdict that comes back the same for every input has stopped
  carrying information, which is the failure mode this project keeps finding.

  Regression tests cover a clause that has actually been through an amendment
  (a freshly adopted v1 clause id is bare, which is why this stayed hidden until
  v2), and the mirror case: a clause from a different goal is still not
  coverage, so the fix cannot pass everything.
