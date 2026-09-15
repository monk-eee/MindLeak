- Work reads now check each task against its own latest scoped event and
  withhold missing-history, missing-source, position, or lifecycle mismatches.
  Publication and task pages share one committed snapshot, including filtered
  and out-of-range pages. Bridge reports HTTP 503 and Industrial MCP reports an
  unavailable tool error; Board Doctor identifies the first repair reason
  without changing data. Detail and tenant-wide unanswered waits are checked
  too. These structural checks preserve synchronous writes and do not claim
  full event replay verification or introduce an asynchronous Work projector.
