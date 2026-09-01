### Added

- Constitution proposals and design materializations now accept and store a
  caller-supplied, optional `display_label` ("who to show in the UI"),
  stored separately from and never substituted for the verified principal
  now recorded as the authoritative `author`/`actor` (ADR-0142 decision 4).
  `industrial_designs`/`industrial_design_decisions` are not yet covered;
  see `gaps.d/design-constitution-display-label-not-stored-separately.md`.
