- **A test server that raced its client turned `main` red on Windows only.**
  `response_decode_failure_opens_the_circuit` checks that a 200 response with a
  body that is not JSON opens the circuit breaker. Its fake endpoint issued a
  single `read` of the request and then let the socket drop. Both halves are
  wrong, and only on Windows does it show. One `read` can return just the
  headers, because a POST body may arrive in a later segment; and dropping a
  socket that still holds unread inbound data makes Windows answer with RST
  rather than FIN, discarding the response the client has not yet read. The
  client then reported a transport failure — `An existing connection was
  forcibly closed by the remote host (os error 10054)`, or `10053` — instead of
  the decode error the test names, so the assertion failed for a reason that
  had nothing to do with the behaviour under test. Measured before the fix: 4
  failures in 12 local runs, green on the ubuntu CI leg and red on
  windows-latest, three consecutive red builds on `main`, and unrelated pull
  requests blocked behind it. The endpoint now reads the request in full,
  honouring the declared `Content-Length`, writes and flushes the response, and
  shuts the write side down before dropping, so the client always gets to read
  what was sent. Verified by running the test 30 times rather than once: 30
  passes, against 8 of 12 before. A single green run cannot tell a fix from
  luck, and for a race that is the only evidence that means anything.
  Production code is untouched — the change is confined to the `#[cfg(test)]`
  helper, so what the breaker does in the field is exactly what it did before.
