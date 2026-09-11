- Node ownership now uses a kernel-held file lock, so a killed process cannot
  strand an otherwise recoverable signer behind a stale marker. Live contenders
  are still refused, and the lock file remains at a stable path after release.
  Added cross-process exclusion/termination tests and native credential recovery
  after an exit that skips destructors. Stop older marker-only node processes
  before upgrading; this does not change enrollment identity or rotate keys.
