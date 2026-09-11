- **Retrying a Work command preview no longer conflicts with its own receipt.**
  The pending-confirmation receipt uses the original command's recorded time
  instead of the retry's wall-clock time. Identical HTTP requests now return the
  original command/receipt identity, while changed payloads and stale task
  versions remain refused. The database-backed browser regression reproduces
  the former HTTP 500 and verifies preview and confirmation replay.
