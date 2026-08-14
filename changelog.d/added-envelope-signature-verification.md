- **Ackplane verifies an evidence envelope's signature before the ledger
  appends it, so a forged or altered record is refused at the boundary rather
  than stored and trusted later.** `sync::translate` already decided whether the
  BYTES were well formed; nothing decided whether the SENDER was who the
  envelope said. `EventEnvelope.signing_key_id` and `signature` were persisted
  and never checked.
  The inbound path now resolves the claimed key through the signing-key registry
  — judged as of the moment the record is accepted, so a key revoked afterwards
  cannot retroactively invalidate what it signed (ADR-0084 decision 12) — and
  verifies the Ed25519 signature over the canonical form ADR-0084 decision 4
  specifies. Refusals are non-retryable and name a distinct cause: an unknown
  key, a key enrolled to a different tenant, repository or node
  (`unauthorized`, not `unauthenticated`, because a real key was presented for
  an identity it does not cover), a key not in force, a signature that does not
  verify, or a missing signature or key id.
  **A declared trust class is refused, never downgraded.** Ackplane can
  substantiate `enrolled_node` and nothing else yet, so `authenticated_principal`
  and `provider_attested` are refused rather than quietly stored as something
  weaker. Downgrading would write a class the producer never claimed and tell it
  nothing, leaving it believing its evidence carried a trust it does not.
  A key registry that cannot answer is reported as retryable `unavailable`,
  because a registry outage is not a node that failed to authenticate.
  The canonical signing bytes are built from the wire envelope rather than the
  translated one: `occurred_at` arrives as an RFC 3339 string and the domain type
  holds a `SystemTime`, so re-formatting it would produce different bytes than
  the node signed — the same instant, a different string — and every signature
  would fail for a reason that looks like forgery.
