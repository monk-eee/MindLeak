- Supervisor lifecycle views now apply distinct receipts with equal
  second-resolution timestamps in their serialized acceptance order, so a
  checkpoint or completion in the start second is not hidden. Exact replays
  and older observations cannot regress the view; receipt history and
  idempotency checks are unchanged. Acceptance order resolves timestamp ties,
  not clock precision or the physical ordering of delayed observations.
