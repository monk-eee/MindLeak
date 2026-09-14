- **Publication cannot record its module-length measurement when the expected
  local control is absent.** Publishing conversational design commit
  `9e345e72ecabc5ae4e74cee5f2c4eda0c4b29583` through
  `scripts/canonical-push.mjs` on 2026-09-14 passed every applicable hook, but
  `scripts/observe-module-length.mjs` received
  `not found: control:rust-module-length`. The optional observation was not
  recorded; this does not mean the source exceeded a measured baseline. Left
  for a separate setup/control-state reconciliation: establish whether this
  repository should have that reviewed control, then repair its declared
  lifecycle or make absence an explicit supported setup state. Do not invent
  a baseline or weaken an existing control merely to suppress the warning.
