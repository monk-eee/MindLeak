- **Amending the constitution orphaned every control bound to the amended clause
  — REPRODUCED, FIXED.** A draft clause is copied as `goal:{slug}@{version}`
  (`copy_clauses_to_version`), so a clause's id changes each time the
  constitution is amended. Controls store the clause id they were registered
  against, and nothing re-pointed them: after `amend_constitution` the old id was
  superseded and every control bound to it answered `control ... serves no
  active clause; an orphan control reports but cannot escalate`. Reproduced
  while giving `source-files-stay-small-and-cohesive` its enforcement contract:
  a ratchet registered before the amendment still evaluated its measurements —
  it even returned `status: fail` for a regression — but the effective
  consequence silently dropped from `review` to `advise`, and
  `clause_controls` for the live clause returned `[]`. — Impact: the failure
  was quiet and the wrong way round. A control that stopped enforcing kept
  answering, so a green-looking `pass` and a real `fail` were equally toothless,
  and the moment it happened was precisely when someone had just taken the
  trouble to strengthen a rule. — Status: fixed. `amend_constitution` now
  carries active controls onto the successor clause inside the same transaction
  that supersedes the old version, so there is no window where a clause is
  active and unguarded. Matched on slug rather than on the outgoing version's
  ids, because an orphan cannot repair itself — re-registering a `control_id` is
  refused once its version has moved forward, so the id is spent — which means
  a control stranded by an earlier amendment is adopted by the next one instead
  of staying orphaned permanently. Retired controls are deliberately left
  naming what they served.
