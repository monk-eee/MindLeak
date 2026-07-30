- **Maintenance-runtime tests now wait for worker progress instead of polling
  SQLite against a two-second wall clock.** A test-only event queue on the
  existing activity condition variable reports when the worker is waiting for
  idle, completes consolidation, or completes pruning. Production state and
  scheduling are unchanged.
  The active-request regression now proves the worker observed the held request
  before release, and the prune-cadence regression holds a request active for
  the entire prune pass. A centralized 30-second wait remains only as a
  deadlock guard when expected progress never arrives, rather than as the
  behavior under test.
