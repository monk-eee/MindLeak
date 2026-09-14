- Serialized concurrent design materializations within one tenant, repository,
  and design. Identical submissions return the original receipt, distinct
  submissions receive unique revisions, and conflicting idempotency-key reuse
  returns a typed conflict rather than a database constraint error. Retry and
  reference reads use the same connection; failed references and cancelled
  writers roll back without consuming a revision. Unrelated designs remain
  writable while another design is locked.
