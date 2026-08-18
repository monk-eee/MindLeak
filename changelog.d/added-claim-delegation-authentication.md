- **`ClaimDelegationService` now authenticates every claim request against the
  enrolled node's signing key, instead of accepting `DelegateClaim`/
  `ReleaseClaim`/`RenewClaim`/`RecoverClaim` from any caller naming any
  `tenant_id`/`repository_id`/`owner_id` with zero authentication** (ADR-0096
  clause 4).
  A new `ClaimAuthentication` message (`signing_key_id`, `node_id`, `signed_at`,
  `nonce`, `signature`) travels on all four requests. `claim_service.rs`
  resolves the claimed key through the same `signing_keys` registry envelope
  signing already uses — judged as of now, never retroactively invalidated by a
  later revocation — and verifies an Ed25519 signature over the claim's own
  identity (tenant, repository, task, owner) with its own domain string, so a
  signature can never be replayed as an envelope or a connection-challenge
  response, or across a different claim.
  An absent, unresolvable, mismatched-binding, not-yet-active, expired,
  retired, or revoked key is refused before the request ever reaches
  `claim_store`'s CAS logic — a binding mismatch (a real key enrolled to a
  different tenant, repository or node) is reported `permission_denied`,
  everything else `unauthenticated`.
  `ClaimStore::resolve_signing_key` mirrors `LedgerStore::resolve_signing_key`
  exactly: the store owns the connection, the decision lives in `signing_keys`
  and stays pure.
