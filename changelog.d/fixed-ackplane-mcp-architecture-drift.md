### Changed

- Fixed `docs/ARCHITECTURE.md`'s `ackplane-mcp` paragraph, stale since
  `open_session` and `active_claims` landed: it described only one tool
  (`check_enrollment_status`) and an authenticated-principal decision as
  "not yet landed," when ADR-0137 clause 2 (session identity) is now
  implemented. Now names all three tools, distinguishes clause 2 (done) from
  clause 1 (connection-level NodeSync authentication, still open), and
  narrows the remaining gaps to ADR-0139 clause 2's `task_query` tool and
  ADR-0140's recall store.
