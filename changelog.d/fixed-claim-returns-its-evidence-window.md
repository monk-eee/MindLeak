- **A won claim now reports the evidence window it opened.** `complete_task`
  refuses evidence whose `started_at` precedes the claim's `claim_started_at`
  with *"evidence interval falls outside the live claim"*, and no tool returned
  that value — `claim_task` gave back only `won` and `governing`, and
  `task_scope` only `paths` and `symbols`. The one number needed to construct
  acceptable evidence was unobtainable, so an agent had to guess a `started_at`,
  and a wrong guess read as a policy refusal rather than a missing accessor.
  `claim_task` now returns `claim_started_at` and `lease_expires_at` on a won
  claim, and reports neither on a lost one (ADR-0060 decision 4).
