### Fixed

- **The fake endpoint in the circuit-breaker test closes gracefully, so Windows
  stops resetting it.** `response_decode_failure_opens_the_circuit` failed on
  every `windows-latest` run, blocking three pull requests at once — and looked
  like a flake because the platform that reproduces it deterministically is not
  the one people develop on.

  It was not a flake. The test's fake endpoint wrote its response and dropped
  the socket while the client's request bytes were still unread, and Windows
  **resets** a connection closed in that state. The client therefore reported
  `os error 10054 — An existing connection was forcibly closed by the remote
  host`: a transport failure, not the invalid JSON the test exists to exercise.

  The endpoint now drains the request before answering, closes the write half
  rather than dropping the socket, and reads to EOF so the client finishes
  first. The assertion also prints the error it actually got, so the next person
  to see this reads the cause instead of `assertion failed`.
