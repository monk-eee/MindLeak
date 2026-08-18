# ADR-0102: Context is compiled into a bounded packet, not assembled by hand

- Status: Proposed
- Date: 2026-08-18
- Deciders: Pending human acceptance
- Related: [ADR-0008](0008-semantic-recall-embedding-index.md) (semantic
  recall — one composed source), [ADR-0017](0017-working-memory-and-autonomous-consolidation.md)
  (working memory / `working_set` — another composed source),
  [ADR-0029](0029-proactive-constitutional-advice.md) (`advise` — another
  composed source), [ADR-0009](0009-evidence-backed-conformance.md)
  (`evidence_for` — another composed source),
  [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md) (the tool surface is a
  contract; this adds to it), [ADR-0022](0022-learned-knowledge-loop.md)
  (precedent for composing existing primitives rather than reimplementing them)

## Context

An informal Phase 1 audit against this repository's own roadmap (2026-08-18)
found that an agent wanting bounded, relevant context today calls `recall`
(semantic entry points), `working_set` (bounded attentional focus), `advise`
(governing clauses), and `evidence_for` (bounded evidence) separately, then
assembles and trims the combination itself. Nothing ranks these four sources
against each other or bounds the combined result by an actual token cost.

The same gap sits directly under a real, current external need. CompLeak, a
product built on top of MindLeak, wants to turn a compliance finding into a
bounded execution packet for a worker agent (AgentD/Agency/Breeze) by
retrieving relevant prior fixes, known failures, repository constraints, and
ownership — precisely MindLeak's job, not the consuming product's, per this
project's own boundary rule. Without a single compilation surface, every such
consumer re-derives its own assembly, ranking, and trimming logic, the same
shape of duplication ADR-0103 separately addresses for the raw protocol client.

Existing bounding precedent already exists in this codebase, but only as a
node **count**, never a token count: `working_set`'s default cap of 7 items,
and `GraphStore::bounded_neighborhood`'s `max_nodes`/`max_fanout` limits. The
cost that actually matters to a consumer assembling a prompt is tokens, not
node count, and nothing today bounds by tokens.

## Decision

**A `compile_context` operation returns one bounded, ranked, token-budgeted
context packet assembled from existing retrieval primitives — it adds no new
source of truth, only composition, ranking, and a budget over sources that
already exist.**

1. **A context packet is a typed, serializable structure**, not free text:
   `facts` (from `recall`/`graph_multi_hop_query`), `working_set` (the calling
   agent's current attentional focus), `governing` (from `advise`, given the
   target node ids), `evidence` (from `evidence_for`, when a task window is
   supplied), and a `budget_report` (tokens requested, tokens used, and what
   was excluded and why). No field is invented data; every field is exactly
   what its existing tool already returns, reshaped into one envelope.
2. **Ranking is one function, reused, never one per caller.** A single
   `rank_for_context(items, now)` scores every candidate the way MindLeak
   already scores structural relevance: decayed effective weight (the
   existing `effective_weight()`) combined with semantic similarity when
   embeddings are available (ADR-0008) — never a new heuristic invented per
   consumer.
3. **The budget is a token count, not a named tier.** The caller passes
   `max_tokens` as a plain integer. The compiler estimates tokens per
   candidate using the same bytes/4 approximation the existing
   "advertised MCP tool surface has a reviewed budget" clause already uses,
   and includes ranked candidates highest-first until the budget is spent —
   it never truncates a candidate mid-item.
4. **Exclusion is explicit, never silent.** `budget_report.excluded` names
   what was cut for budget reasons alone (id and rank), distinct from what
   never matched at all — the same "unknown is not an all-clear" discipline
   `check_overlap` already applies to paths the graph has never seen.
5. **Compilation calls existing tools internally; it duplicates none of
   them.** `compile_context` is implemented as a facade method composing
   `recall`, `working_set`, `advise`, and optionally `evidence_for` — each
   remains independently callable for a consumer that wants only one. This
   mirrors how `promote_signals` already composes `promotion_candidates` and
   `consolidate_signal` rather than reimplementing either (ADR-0022).
6. **No model call is required to compile a packet.** Ranking, budgeting, and
   assembly are pure deterministic composition over already-computed scores.
   An optional narration pass may summarise the packet afterward through the
   existing consolidation client, exactly as digest compilation does (ADR-0101);
   failure of that optional step never blocks returning the packet itself.

## Consequences

- External consumers — CompLeak today, any future MCP agent tomorrow — get one
  call instead of four, with a token budget they control, directly removing
  the hand-assembled-prompt failure mode the original roadmap and the
  CompLeak design discussion both named.
- `compile_context`'s output shape becomes a new stable contract surface,
  subject to ADR-0059's "tool descriptions are a contract, not a narrative"
  discipline and the reviewed tool-surface-budget clause, like any other tool.
- One moderately complex new tool rather than four simple ones, reviewed
  against the tool-surface-budget clause at implementation time.
- Token estimation stays an approximation (bytes/4), matching the existing
  convention in the tool-surface-budget clause rather than introducing a
  stricter standard nothing else in the project meets.

## Rejected alternatives

- Fixed named budget tiers (1000/5000/10000, as the informal roadmap sketch
  proposed) — rejected: an arbitrary tier is either too coarse for a small
  task or wastes budget on a large one; a plain integer lets every caller
  state its actual limit.
- Building this only inside CompLeak — rejected under this project's own
  boundary rule: a capability useful to any MCP agent regardless of domain
  belongs upstream, not duplicated per consumer.
- A new bespoke ranking heuristic independent of `effective_weight`/semantic
  similarity — rejected: MindLeak already has two independently-evolved
  relevance signals; a third invented one would compete with, rather than
  reuse, them.
