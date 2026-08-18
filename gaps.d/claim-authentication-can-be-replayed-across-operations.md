- **A valid `ClaimAuthentication` can be replayed for a different claim RPC or
  changed payload because the signature proves only tenant/repository/task/owner,
  and its freshness fields are never enforced - found in
  `crates/ackplane-server/src/claim_signature.rs::claim_signing_bytes` and
  `verify`, left OPEN.** The signed bytes omit the operation (`DelegateClaim`,
  `RenewClaim`, `ReleaseClaim`, or `RecoverClaim`) and every operation-specific
  field: lease duration, branch, paths, symbols, expected owner, and recovery
  reason. The same captured authentication therefore verifies for another RPC
  over the same task and owner, including a release, or for changed lease/scope
  values. `signed_at` and `nonce` are covered by the signature but are not
  parsed, bounded, persisted, or consumed, so the same request can also be
  replayed indefinitely. This does not invalidate the identity proof delivered
  by PR #513, and no production client routes claims yet, but it must be fixed
  before authenticated federated claim routing ships. The fix needs one
  canonical operation-specific signing contract shared by client and server,
  plus a durable freshness/replay decision (for example, bounded timestamps and
  single-use nonce consumption); it is not part of the client arbitration test
  fixture repair.
