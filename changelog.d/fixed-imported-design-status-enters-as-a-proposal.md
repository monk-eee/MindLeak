- **An imported ADR status can no longer assert a decision nobody made.**
  Reconciliation used to import an ADR file's `accepted`/`rejected` status at
  face value, creating a design row that recorded a decision with an empty
  `decided_by`. A newly discovered row now always enters as `proposed`; only the
  explicit Design Board decision path can accept or reject it (ADR-0077). Rows a
  person already decided are untouched, and reconciliation stays idempotent.
