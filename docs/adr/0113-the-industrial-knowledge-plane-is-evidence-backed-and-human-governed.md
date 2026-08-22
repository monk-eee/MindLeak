# ADR-0113: The Industrial knowledge plane is evidence-backed and human-governed

- Status: Accepted
- Date: 2026-08-21
- Deciders: MindLeak maintainers
- Accepted: 2026-08-21 by the repository owner, authorized directly in session
   — attributed human adoption after review.
- Depends on: [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (PostgreSQL is Ackplane's durable authority),
  [ADR-0106](0106-ackplane-closes-the-agentic-operating-loop.md) (the
  Industrial operating loop)
- Related: [ADR-0072](0072-an-advisory-informs-it-does-not-cap-the-verdict.md)
  (advice does not replace authority),
  [ADR-0102](0102-context-is-compiled-not-assembled-by-hand.md) (bounded
  context), [ADR-0108](0108-knowledge-rpcs-authenticate-with-operation-signing.md)
  (authenticated knowledge requests)

## Context

An Industrial fleet needs to retain what it learns across laptops, cloud
workers, pipelines, agent versions, and human shifts. A vector index alone
does not provide that capability. It can rank text that looks similar, but it
cannot answer who supplied a statement, what evidence supports it, which
repository it applies to, whether it remains fresh, or whether a person ever
authorized it to influence work.

The existing PostgreSQL-backed knowledge slice deliberately starts small:
record, recall, and retire a statement. That is useful retrieval plumbing, but
it is not yet a complete knowledge plane. Without an explicit lifecycle,
unverified observations can look like fleet guidance, stale guidance can keep
returning because its embedding is similar, and a useful recommendation can be
mistaken for a policy decision.

The human role matters here. Humans cannot review every observation produced by
a large fleet, but they must retain control over the rules by which an
observation becomes reusable organizational guidance, the scope within which it
may be used, and the exceptions an agent may take. The system needs to learn
quickly without allowing accumulated correlations to become law by accident.

## Decision

**Ackplane's knowledge plane stores evidence-backed, scoped, revalidatable
knowledge as a durable domain. It ranks and explains guidance, but it never
creates authority without a human-approved policy or a human decision.**

1. **Knowledge has an explicit lifecycle, not one undifferentiated active
   bucket.** An authenticated node, agent, or authorized service may record an
   `observation` or `candidate`. A candidate becomes `active` only through an
   explicit human review or an adopted policy that names the evidence,
   corroboration, and scope conditions required for that class of knowledge.
   Active knowledge may later be `reconfirmed`, `revised`, `superseded`, or
   `retired`. Supersession preserves the prior statement and records why the
   replacement won; retirement preserves the statement and makes its non-use
   visible. No update rewrites the historical statement or its provenance in
   place.

2. **Every knowledge record is structurally scoped and attributable.** Its
   durable identity includes tenant, repository and, where applicable, project
   or product scope; authoring principal or node; creation and confirmation
   times; lifecycle state; source evidence references; source diversity;
   reach; and links to the policy or human review that activated it. A record
   with no applicable scope is not silently treated as tenant-wide. Cross-
   repository reuse requires an explicit tenant or product scope and a policy
   that authorizes that reach.

3. **Evidence and outcomes determine trust; an embedding does not.** The
   knowledge plane retains bounded evidence and outcome references, including
   the task, context packet, validation, or receipt that supported a lesson.
   It records corroboration and later contradiction as separate facts. A
   semantic similarity score may rank an eligible record, but it cannot
   promote a candidate, extend its scope, or override a retirement decision.
   A single opaque confidence number is not a substitute for showing the
   supporting evidence, freshness, and activation basis.

4. **Relevance decays and revalidation is visible.** Effective relevance is
   derived at query time from confirmed time, half-life, lifecycle state, and
   any policy-defined revalidation rule; it is never written back as a mutable
   score. The system surfaces records approaching expiry, records contradicted
   by later evidence, and records whose required revalidation is overdue in a
   review queue. A stale record may remain auditable, but it is marked stale
   and is not silently presented as current guidance.

5. **Retrieval is explained, bounded, and authorization-aware.** A knowledge
   query returns only records the requester may read for the requested tenant
   and scope. Each result carries its lifecycle state, effective relevance,
   evidence/provenance references, freshness, and the reason it was selected,
   such as lexical match, vector similarity, structural reach, or an explicit
   task reference. Queries have hard result, payload, and traversal bounds.
   `pgvector` is a retrieval accelerator, not a source of truth or a policy
   engine.

6. **Knowledge content is minimized and governed as data.** Ackplane stores
   the smallest statement and references sufficient for reuse. Raw source,
   credentials, unrestricted terminal logs, private prompts, and complete
   local graph databases remain outside the knowledge domain unless a future
   data contract explicitly permits them. Access control, retention, export,
   and deletion follow tenant and repository policy. Operational telemetry
   records identifiers, state, and digests rather than sensitive statement
   bodies.

7. **Lifecycle actions are typed, authenticated, and receipted.** Recording,
   reviewing, reconfirming, revising, superseding, and retiring knowledge use
   versioned contracts bound to their operation-specific fields and requester
   identity. Every accepted lifecycle change has an immutable receipt naming
   the prior state, new state, actor, reason, evidence references, and
   authorization basis. A failed authorization, stale version, tenant mismatch,
   or missing evidence returns a typed refusal and changes nothing.

8. **Model output is a candidate, never evidence or authority.** A configured
   model may cluster observations, draft a candidate lesson, summarize a
   bounded packet, or suggest revalidation. Its model identity, prompt-input
   references, and output digest are retained as provenance. It cannot assert
   that a source was observed, activate knowledge, alter Constitution or
   policy, grant a waiver, or issue a control directive unless a separately
   authorized human or adopted policy acts through its own contract.

9. **Knowledge informs a context packet rather than becoming a global prompt
   dump.** The context compiler selects scoped active knowledge for a named
   task, agent session, and budget; it retains why each item was included or
   excluded. A later context-packet decision defines the durable packet
   contract. No agent receives every record simply because it belongs to the
   same tenant.

## Consequences

- The existing knowledge store grows into a lifecycle domain with candidate,
  review, active, stale, superseded, and retired projections; pgvector remains
  one query input rather than the lifecycle authority.
- Operators gain a Bridge revalidation and provenance workflow: they can see
  what the fleet believes, why it believes it, which statements need review,
  and what has been retired or contradicted.
- Agents can contribute useful observations at fleet scale without receiving
  the authority to turn those observations into policy or unrestricted work
  instructions.
- Every context decision can later explain whether it relied on active,
  revalidated knowledge or on a record that was already stale or superseded.
- The domain needs real PostgreSQL tests for tenant isolation, scope reach,
  lifecycle races, revalidation, evidence integrity, authorization, and
  retention behavior before it can be represented as an Industrial capability.

## Rejected alternatives

**Treat every embedded string as active organizational memory.** Rejected
because semantic similarity cannot prove scope, provenance, freshness, or
authorization, and makes stale or malicious text look authoritative.

**Require a person to hand-approve every observation.** Rejected because a
large fleet would produce more observations than a human can review. The
correct scaling boundary is human-approved activation policy and explicit
exception review, not removing humans or making them a per-event bottleneck.

**Allow a model to promote its own summaries after repeated agreement.**
Rejected because repeated model agreement is not independent evidence and a
model has no constitutional or human authority.

**Store full source trees, terminal transcripts, and prompts so retrieval has
more text.** Rejected because it silently turns a knowledge plane into a
cross-machine data-exfiltration system and makes access, retention, and
evidence claims impossible to reason about.
