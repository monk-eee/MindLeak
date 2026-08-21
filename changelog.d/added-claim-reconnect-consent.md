### Added
- `task_claim`'s `renew` step now takes an opt-in `reconnect_clause` argument
  (ADR-0109): after a successful renewal, the current owner of a live claim
  may ask to move their own task's `goal_id` from a superseded clause onto
  its active same-slug successor. Refused, with a distinct reason, unless the
  caller currently holds the live claim and exactly one such successor
  exists. The move is one attributed, append-only event on the task's
  thread; it never re-audits, deletes, or relabels a conformance record
  already written under the outgoing clause. Closes the gap where a task
  spanning a constitutional amendment had no route to `aligned` for the rest
  of its claim window short of releasing the claim (which holes the evidence
  window) or a human overruling a `drift` verdict.
