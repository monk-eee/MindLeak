- Ackplane's `EnrollmentStore::submit` bound a node's declared `created_at`
  and `expires_at` (RFC3339 text) straight into a `timestamptz` column with a
  SQL-side `::timestamptz` cast. Postgres describes that parameter's type from
  the prepared statement, so it is `timestamptz`, not text, and the driver
  rejected every bound `String` before the query ever ran. Every enrollment
  submission against a real database failed, taking every downstream
  Postgres-gated enrollment test down with it (approval, activation, and now
  rotation) — none of it had ever actually been exercised against a real
  Postgres. Fixed by parsing the timestamp in Rust before binding it, with a
  clear, request-scoped error for a value that does not parse.
