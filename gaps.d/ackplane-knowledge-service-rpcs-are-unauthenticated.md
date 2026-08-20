- **Ackplane's `KnowledgeService` RPCs (`RecordKnowledge`, `RecallKnowledge`,
  `RetireKnowledge`) ship unauthenticated in this first slice.** Unlike
  `ClaimDelegationService`, no request is bound to the enrolled node's
  signing key, and no nonce or nonce-window guards a replay. This was a
  deliberate scope decision, not an oversight: `ClaimOperation`
  (`crates/ackplane-protocol/src/claim_auth.rs`) and its `CLAIM_DOMAIN`
  separator are scoped to claim-specific fields (`task_id`, `owner_id`,
  `branch`, `lease_seconds`, ...) that have no equivalent for a knowledge
  statement, so reusing that scheme for this domain would have been a
  domain mismatch, not reuse — and designing a second, knowledge-scoped
  signing domain from scratch would have roughly doubled this slice's
  scope for a first cut whose main job was proving the pgvector-backed
  decay and recall path for real.

  Impact: any caller that can reach `ackplane-server`'s gRPC port can
  record, recall, or retire knowledge for any `(tenant_id, repository_id)`
  pair it names, with no proof it is the node it claims to be. This is the
  same trust boundary the rest of Ackplane's federation surface closes
  (see ADR-0096, `NodeEnrollmentService`, `ClaimDelegationService`) --
  knowledge is simply the one domain that does not yet sit behind it.

  What would close it: a knowledge-scoped operation-signing scheme
  mirroring `ClaimOperation`/`claim_signature.rs` (its own domain
  separator, its own nonce table keyed off `knowledge_id`/`tenant_id`/
  `repository_id` rather than claim fields), wired the same way
  `claim_service.rs` verifies before it ever touches `ClaimStore`. Until
  that lands, do not point a production or multi-tenant-sensitive
  deployment's `KnowledgeService` at an untrusted network.
