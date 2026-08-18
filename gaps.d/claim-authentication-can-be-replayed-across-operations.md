- **A valid `ClaimAuthentication` can still be replayed for a *different*
  claim RPC or changed payload, because the signed bytes cover only
  tenant/repository/task/owner and omit the operation and its
  operation-specific fields - found in
  `crates/ackplane-server/src/claim_signature.rs::claim_signing_bytes`, left
  OPEN.** The signed bytes omit the operation (`DelegateClaim`, `RenewClaim`,
  `ReleaseClaim`, or `RecoverClaim`) and every operation-specific field: lease
  duration, branch, paths, symbols, expected owner, and recovery reason. The
  same captured authentication therefore still verifies for another RPC over
  the same task and owner, including a release, or for changed lease/scope
  values, even though it can no longer simply be resent unchanged (see below).
  This does not invalidate the identity proof delivered by PR #513, and no
  production client routes claims yet, but it must be fixed before
  authenticated federated claim routing ships. The fix needs one canonical
  operation-specific signing contract shared by client and server; it is not
  part of the client arbitration test fixture repair, and — to keep this
  follow-on entirely out of `crates/ackplane-client/**` while that crate is
  under a separate live claim — was not attempted here either.

  **CLOSED (this task): the freshness/replay half.** `signed_at` and `nonce`
  were carried in the signed bytes since PR #513 but never parsed, bounded,
  persisted, or consumed, so the *exact same* request could be resent
  indefinitely. `claim_signature::verify` now takes a `now: SystemTime` and
  refuses a `signed_at` outside a bounded (5-minute) clock-skew window before
  any signature or database work runs (`ClaimAuthRefusal::MalformedTimestamp`
  / `StaleTimestamp`), and `ClaimStore::consume_claim_nonce` durably consumes
  each `(signing_key_id, nonce)` pair exactly once via
  `migrations/0006_claim_authentication_nonces.sql`, refusing a repeat as
  `ClaimAuthRefusal::Replayed`. Entirely server-side (`ackplane-server` only);
  the bytes being bound were already part of the wire contract, so no
  `ackplane-client` or `ackplane-protocol` change was needed.
