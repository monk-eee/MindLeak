### Added

- Bridge repository `claims` and `stranded-claims` endpoints now use the
  compound keyset pagination required by ADR-0112. Callers pass
  `after_lease_expires_at_micros` and `after_task_id` together, then follow
  the response's `next_after` value until it is `null`, rather than losing
  every claim after the former fixed 50-row slice. Ordering by lease expiry
  and task id keeps page boundaries unambiguous when several claims expire at
  the same time.
