# ADR-0087: The Ackplane graph is a projection, not an authority

- Status: Accepted
- Date: 2026-08-10
- Deciders: MindLeak maintainers
- Accepted: 2026-08-10 by the repository owner — attributed human adoption after
  review.
- Depends on: [ADR-0086](0086-postgresql-is-the-backplane-ledger-and-arbiter.md)
  (PostgreSQL ledger and projections)
- Related: [ADR-0002](0002-sqlite-decay-over-vector-llm.md) (decay graph over
  vector-only memory), [ADR-0007](0007-structural-snapshot-reconciliation.md)
  (owned structural facts), [ADR-0008](0008-semantic-recall-embedding-index.md)
  (similarity seeds traversal), [ADR-0053](0053-the-graph-records-events-not-conclusions.md)
  (events, not conclusions), [ADR-0084](0084-backplane-evidence-has-explicit-trust.md)
  (evidence minimisation)

## Context

The Bridge wants questions the ledger alone answers badly: what a change
can affect, which repositories share an ownership boundary, where an agent's
footprint overlaps another's, and where a policy question was answered before.
Those are graph and similarity questions, and PostgreSQL can host both through
extensions — `pgvector` for embeddings, Apache AGE for openCypher traversal.

The attraction is real, and so is the failure mode. MindLeak already has a graph
authority: the repository-local decay graph, whose structural facts are owned
and replaced by their artifacts (ADR-0007) and whose relevance is derived rather
than stored. Standing up a second graph engine that accepts writes would create
two engines that can disagree about what the code looks like, with no rule for
which one is right. It would also invite mirroring the entire local graph into a
shared service, which conflicts with evidence minimisation.

Extension availability is an equally practical constraint. `pgvector` is widely
supported by managed PostgreSQL services. Apache AGE is not: several managed
offerings do not provide it, and it tracks PostgreSQL major versions on its own
schedule. A design that requires AGE quietly decides where the product can be
deployed.

## Decision

1. **Graph and vector tables are projections of accepted ledger records.** They
   are derived, rebuildable, and versioned under ADR-0086 decision 7. Nothing
   writes to them except a projector reading committed records. Dropping and
   rebuilding a projection from the ledger must reproduce it exactly; that
   rebuild-and-diff is a required test.

2. **The portable baseline is ordinary SQL.** Projected nodes and edges are
   normalised, indexed relational tables, and traversal is a recursive CTE with
   explicit depth, fanout, and node-count bounds. Every supported deployment has
   this. No capability depends on an extension being present.

3. **MindLeak owns the traversal contract, not the extension.** Ackplane
   traversal keeps the bounded, best-first semantics the local engine already
   uses: seed set, maximum depth, maximum admitted nodes, per-node fanout
   limited to the strongest edges, and dangling edges dropped. Any backend must
   satisfy that contract and return the same set for the same input, ordering
   ties aside.

4. **Apache AGE is a sanctioned optional accelerator, not a dependency, and is
   not built until it is needed.** It may be adopted when measured p95 latency
   for the contracted traversal exceeds the interactive budget at a deployment's
   real projection size and indexing and query tuning have not closed the gap.
   Adoption then requires a capability probe at startup, automatic fallback to
   the SQL baseline, and an equivalence test asserting identical results from
   both backends on the same fixtures. A deployment without AGE loses speed,
   never an answer.

5. **`pgvector` is the sanctioned vector index when semantic search lands.**
   Exact scan remains correct for small projections and is the fallback where
   the extension is unavailable. Vectors seed traversal and rank candidates;
   they never substitute for a structural relationship, matching ADR-0008.

6. **Effective weight stays derived at query time.** Projected edges carry their
   base weight, half-life, and timestamps, and decay is computed in the query.
   No background job rewrites stored weights, and no projection freezes a decay
   result into a column that then ages silently.

7. **Only minimised, published records are projected.** The projection is built
   from what a node deliberately published under ADR-0084 decision 10 — bounded
   structural summaries, evidence references, and identities. It is not a mirror
   of the local decay graph, and it never contains source text, terminal output,
   or raw episodic logs.

8. **Embeddings never leave the deployment silently.** Vectors are computed
   locally or by an endpoint the operator configured explicitly. Ackplane does
   not send projected content to a third-party embedding service by default, and
   a model-derived value is recorded as derived, never as evidence.

9. **Cross-repository traversal is analysis, not authority.** A query may span
   repositories inside one tenant to answer an impact or ownership question. It
   creates no claim, no coordination edge, and no cross-repository task
   relationship; ADR-0082 decision 9 still governs what Ackplane arbitrates.

10. **Projection freshness is visible.** Every graph or similarity answer carries
    the ledger position and projection time behind it, so a stale or still
    rebuilding projection is legible rather than presented as current fact.

## Consequences

- Ackplane can answer blast-radius, ownership, and overlap questions across a
  tenant without becoming a competing source of truth for code structure.
- A rebuildable projection can be corrected by replaying the ledger, which is
  the cheapest possible recovery from a projector defect.
- The initial system carries one traversal implementation. The extension door is
  designed open, with a stated trigger, rather than paying for two query engines
  and their equivalence tests before a measurement justifies them.
- Managed PostgreSQL remains viable, because `pgvector` is optional for
  correctness and AGE is optional entirely.
- Recursive CTE traversal will need deliberate indexing and bounded inputs. An
  unbounded traversal request is refused rather than served slowly.
- Keeping decay derived means graph queries pay a small compute cost per row
  instead of accumulating a silent correctness debt in stored weights.

## Rejected alternatives

**Add a dedicated graph database beside PostgreSQL.** Rejected because it
creates a second store to keep consistent with the ledger, a second failure and
backup domain, and a distributed-transaction question at exactly the records
where correctness matters most.

**Require Apache AGE.** Rejected because it constrains supported deployments and
couples the product to an extension's major-version cadence, in exchange for
query ergonomics rather than a capability the baseline lacks.

**Replicate the whole local decay graph into Ackplane.** Rejected because it
contradicts evidence minimisation, uploads local implementation detail, and
would make the shared service grow with every repository's episodic churn.

**Store computed effective weight in the projection.** Rejected because a
derived value written down becomes wrong the moment it is written, and a job
that refreshes it re-implements decay in a second place.

**Let the projection accept direct writes for convenience.** Rejected because a
projection that can be written is an authority, and two authorities over the
same facts is the split-brain this architecture keeps refusing.
