- **Single-owner supervisor state:** startup now holds a process-lifetime
  ownership lock before checking recovery markers or connecting to Ackplane.
  A second supervisor using the same state directory is refused even while
  both are idle. Independent directories and concurrent slots remain supported;
  process exit releases ownership without deleting recovery evidence. This does
  not automatically recover active orphan workers or arbitrate workspaces
  configured under separate state directories.
