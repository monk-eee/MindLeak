### Added

- Added `ackplane-supervisor`, a local SQLite-backed durable inbox for one
  enrolled supervisor session. It validates directive target identity,
  advertised capability, RFC3339 expiry, and contiguous sequencing; persists
  accepted, capability-refused, and expired receipts; returns the original
  durable receipt for an identical replay; and refuses same-id changed-digest
  replay without overwriting evidence. The crate deliberately does not open a
  listener, launch workers, execute directives, or expose a shell.
