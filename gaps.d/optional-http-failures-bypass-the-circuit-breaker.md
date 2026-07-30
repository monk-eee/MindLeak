- **Optional HTTP failures outside `send_json` bypass the circuit breaker --
  REPRODUCED, OPEN.** In `net::post_json_with_cancel`, the breaker allows the
  call and then `resolve_endpoint(...)?` returns DNS failures before
  `record_failure` can run. The successful-status path has the same hole:
  `read_bounded_json(...)?` returns invalid or oversized responses without
  recording either success or failure. These are degraded-endpoint outcomes,
  but they never contribute to the consecutive-failure threshold promised by
  ADR-0010.

  Reproduced through the public MCP surface on 2026-07-30 from main
  `1f82ef811a16f9948a758beb24d690bb4f98c4d1`, and verified unchanged on main
  `fd176c196164196c6e3e02381fa5a510e2957d83`. With
  `MINDLEAK_BREAKER_THRESHOLD=1`, a 60-second cooldown, and
  `MINDLEAK_EMBED_URL=http://does-not-resolve.invalid:11434/v1`, two
  consecutive `index(limit=1)` calls both performed resolution and returned
  `DNS resolution failed`; the second should have fast-failed with `circuit
  open`. A local HTTP server returning status 200 with `not json` produced the
  same result: the second call reached the server and repeated the JSON error.

  Impact is confined to optional embeddings and consolidation, but persistent
  DNS or response corruption can consume the full timeout on every call and
  can leave another resolver thread outstanding until the OS resolver returns.
  The breaker tests exercise `CircuitBreaker` in isolation and do not cover
  these early-return paths. Left open: every endpoint-level failure after
  `breaker_allows` must record failure exactly once before it is returned, with
  an integration test proving threshold-one fast-fail for resolution and
  response-decoding errors.
