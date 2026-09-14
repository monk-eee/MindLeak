- **PostgreSQL connection startup is bounded:** the pool timeout now limits
  creating a connection as well as waiting for a free slot. A server that accepts
  TCP but never completes its startup handshake returns a typed creation timeout
  and releases pool capacity. `ACKPLANE_DB_POOL_TIMEOUT_MS` applies separately to
  those phases; it is not an end-to-end request or SQL-query deadline.
