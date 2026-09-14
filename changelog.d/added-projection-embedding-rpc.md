- **Authenticated server embedding ingestion:** enrolled nodes can list projected
  labels needing vectors and publish embeddings through the node companion.
  Requests sign the exact scope, model, source label and vector in a separate
  domain with durable replay protection. Stale-source writes, forged requests,
  revoked keys and cross-repository access are refused. Whole-source pages are
  bounded to fit the companion transport. Ackplane makes no model calls; the
  automatic node-side indexing loop and shared recall surface remain unfinished.
