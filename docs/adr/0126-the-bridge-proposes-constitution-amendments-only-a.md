# ADR-0126: The Bridge proposes constitution amendments; only a repository activates them

- Status: Accepted
- Date: 2026-08-25
- Deciders: MindLeak maintainers
- Accepted: 2026-08-25 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Refines: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  decision 4 (policy distribution cannot become policy activation),
  [ADR-0121](0121-industrial-design-preserves-immutable-history.md) decision 2
  (Ackplane remains a projection, never the editor of a Local Constitution)
- Related: [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (the constitutional policy model this proposes into),
  [ADR-0043](0043-adoption-into-active-constitution-is-an-amendment.md) (the
  local attributed amendment flow this defers to),
  [ADR-0045](0045-a-fleet-is-a-distributed-system.md) (one arbiter per shared
  resource — the invariant this ADR does not touch),
  [ADR-0106](0106-ackplane-closes-the-agentic-operating-loop.md) (the read
  projection this extends)

## Context

`GET /api/v1/repositories/:id/constitution` (ADR-0106 decision 3) is the
Bridge's only constitution-facing route today, and it is a pure read: a
`ConstitutionResponse` built from `ConstitutionStore::get_active`, wired to
one static HTML page whose only form loads a repository, nothing else.
`crates/ackplane-server/src/constitution_store/mod.rs` says as much in its
own header: *"No adopt/tailor/reject/promote/waiver action lives here."*

That absence is not an oversight. ADR-0082 decision 4 states it directly:
*"Policy distribution cannot become policy activation... Ackplane has no
shorter activation path. An organisation administrator cannot turn a
proposal into active repository law merely by changing an Ackplane
setting."* ADR-0121 decision 2 restates the same boundary three months later
for a different feature touching the same seam: *"Ackplane remains a
projection, never the editor of a Local Constitution... it never reads a
repository filesystem, invokes Local MCP, rewrites an ADR, or executes
Local `accept_design`/`reject_design`/`retire_design`."* The reason named in
both places is ADR-0045's one-arbiter-per-resource rule: if Ackplane could
also activate policy, two things could each claim to be a repository's law,
and whichever wrote last would win by accident of timing rather than by
review.

That reasoning is sound and this ADR does not reverse it. But it leaves a
real, named gap: an operator watching many repositories through the Bridge
has no way to *originate* a suggested governance change from where they are
actually looking. Today that requires being inside the target repository's
own worktree, calling Lodestar tools directly. ADR-0082 decision 4 already
permits exactly this kind of thing for versioned policy packs authored
upstream (*"Ackplane may distribute immutable policy packs and amendment
proposals"*) — this ADR asks for the same shape, for an ad hoc single-clause
suggestion authored from the Bridge instead of only a published pack
version.

## Decision

1. **Ackplane gains an append-only `constitution_proposals` table**, unique on
   `(tenant_id, repository_id, proposal_id)`. Each row carries a suggested
   clause change in the exact `ClauseSnapshot` shape the read projection
   already returns (kind, slug, title, statement, consequence, scope,
   rationale) — no new clause type, so the Bridge UI can diff a proposal
   against the active snapshot with nothing new to reconcile. Each row also
   carries an attributed author label, `created_at`, and a status of
   `proposed` or `withdrawn`.

2. **`POST /api/v1/repositories/:id/constitution/proposals` is the only new
   write path**, and it only ever inserts a `constitution_proposals` row. It
   never writes `constitution_publications` (the authoritative table), never
   reaches a repository's filesystem, and never invokes Local MCP — the exact
   boundary ADR-0121 decision 2 already states, extended to this new object
   rather than crossed by it.

3. **A proposal is append-only and immutable once authored.** The only
   mutation it ever receives is `status: withdrawn`, by its own author. A
   changed idea is a new proposal, not an edit — the same rule ADR-0121
   decision 1 already applies to publications, carried down to this lighter
   object so its history stays literal.

4. **A repository learns about a pending proposal the same way it already
   learns about everything else it should look at:** `open_session` gains a
   `pending_bridge_proposals` field, populated only when this repository is
   `federated` (ADR-0082 decision 3) and enrolled with a live Ackplane
   connection reporting open proposals for its `repository_id`. A
   disconnected or `local`-mode repository sees nothing and behaves
   identically to today — matching ADR-0082 decision 6's disconnection story
   without exception.

5. **Adoption is entirely the existing local flow, unmodified.** An agent or
   human reviewing a surfaced proposal runs `propose_amendment` ->
   `draft_clause` (using the proposal's suggested text as a starting point,
   editable like any other draft) -> `amend_constitution`, citing the
   proposal id in the amendment's own rationale. Ackplane observes this only
   indirectly: when the repository's own next constitution publication
   references the proposal id it resolved, the Bridge correlates the two and
   displays the proposal as `adopted`; a proposal `withdrawn` without ever
   being referenced displays as such. No new authority is granted to make
   this correlation — it is read-only pattern matching over two append-only
   tables the system already has.

6. **The same route surfaces which policy pack(s) a repository has adopted,
   read-only, using the existing `PackUpgrade`/`PackUpgradeClause`
   comparison already built for the local amendment facade** (`facade::
   amendments`). This answers the "different constitution flavours per
   repository, cloned from a common default" question directly: the Common
   Core and Extension Packs (ADR-0026 decisions 6-7) already *are* that
   mechanism — a repository adopts a pack version, tailors it locally, and
   upstream changes surface as reviewable amendment proposals rather than
   silent inheritance. What was missing was visibility from the Bridge, not
   a new mechanism; a pack-version mismatch surfaced here becomes a
   `constitution_proposals` row exactly like a hand-authored one, through
   the same route as decision 2, not a second path.

7. **A proposal is advice, never a gate.** It carries no lease, holds no
   claim, and blocks no work. It ages out of the Bridge's default view after
   a fixed number of days without being referenced, but is never deleted —
   the same append-only spirit as every other domain this Ackplane server
   already hosts.

## Consequences

- An operator can browse every enrolled repository's constitution and
  originate a suggested change, or notice an available pack upgrade, from
  one place, without a clone.
- ADR-0045's one-arbiter rule holds without exception: the only call that
  ever changes what is active, anywhere, is a local `amend_constitution`.
- A proposal can go stale relative to the constitution it was drafted
  against, if the repository amends independently in the meantime. The
  Bridge UI must diff a proposal against the *current* active snapshot at
  review time, not the one captured when it was authored, or a reviewer
  could approve a diff against an already-superseded baseline without
  noticing.
- New surface, deliberately the smallest slice that delivers the actual
  requested experience: one Postgres table, one write route, one
  `open_session` field, one read-only pack-comparison view. Multi-clause
  batch proposals, reviewer sign-off routing, and proposal categorisation
  are explicitly deferred rather than designed speculatively.
- Attribution of the *author* of a Bridge-originated proposal is honest but
  weak until ADR-0098/OIDC lands: a label, not a verified principal — the
  same limit every other Bridge mutation admits today (ADR-0111's
  precedent). This ADR does not raise or lower that bar.

## Rejected alternatives

**Let the Bridge write `constitution_publications` directly, applied on the
repository's next sync.** This is exactly the "shorter activation path"
ADR-0082 decision 4 already names and forbids. Two writers of "what is
active" is the split authority ADR-0045 exists to prevent, and ADR-0082
decision 6's disconnected-repository semantics would stop holding, since a
change could originate and apply while the repository itself was never in
the loop.

**Have the Bridge open a remote MCP connection into the target repository's
own `lodestar-mcp` and call `amend_constitution` on its behalf.** Rejected
for the same reason ADR-0082 decision 2 already rejects mounting or
replicating local databases: it would require Ackplane to hold reachability
or credentials into an arbitrary developer machine, crossing the network
boundary the local planes' stdio-only design deliberately avoids.

**Treat a pack-upgrade mismatch as automatically applied unless a repository
opts out.** Rejected because it inverts ADR-0026 decision 7's stated rule —
*"upstream updates produce amendment proposals; they never mutate active
local policy"* — from an explicit choice into a default, which is precisely
the silent-inheritance risk that decision exists to prevent.
