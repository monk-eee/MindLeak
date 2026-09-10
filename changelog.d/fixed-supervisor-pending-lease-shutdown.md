- **Shutdown during assignment preparation:** the supervisor now tracks confirmed
  task leases independently from worker processes. Stopping while a context reply
  is pending releases those leases without starting a worker or reporting a false
  lifecycle; transient release failures remain retryable. Normal worker shutdown
  still stops the owned process group before releasing its lease. Unconfirmed
  grants and abrupt process-loss recovery are unchanged.
