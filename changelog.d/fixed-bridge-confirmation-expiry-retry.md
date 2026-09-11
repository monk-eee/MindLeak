- **A lost Work confirmation response remains recoverable after expiry.**
  The browser retries the original immutable command after an ambiguous attempt
  so the server can return its receipt without duplicating the effect. A first
  confirmation after expiry remains blocked, and a server refusal is never
  presented as success. A regression covers both an already-applied receipt and
  an expired command that did not execute.
