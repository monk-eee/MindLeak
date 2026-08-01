# ADR-0080: Knowledge is searched where it is already read

- Status: Accepted
- Date: 2026-08-01
- Deciders: monk-eee
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (the Intent Plane is
  separate from the decay graph), [ADR-0008](0008-semantic-recall-embedding-index.md)
  (optional semantic recall over the graph),
  [ADR-0022](0022-learned-knowledge-loop.md) (the learned-knowledge loop),
  [ADR-0053](0053-the-graph-records-events-not-conclusions.md) (the graph records
  events, not conclusions); the gap fragment
  `gaps.d/complete-task-learned-writes-conclusions-where-recall-cannot.md`

## Context

ADR-0053 decision 4 says `record_knowledge` and `record_architectural_decision`
both embed the node they write. Only the second was delivered.
`record_architectural_decision` writes a MindLeak intent node and embeds it, so
it is recallable immediately. `record_knowledge` — which is what
`complete_task(learned:)` actually calls — writes to the Lodestar `knowledge`
table, which has no embedder and no vector index. `recall` searches MindLeak, so
it cannot see any of it.

The write path most likely to be used is the one that rides on finishing work,
and it is exactly the one whose output retrieval cannot reach.

Two delivery paths do work, and both are exact rather than semantic: the
conformance advisory matches recorded knowledge on **referenced nodes**, and a
second path matches on the **goal** the lesson was learned under. Measured over
214 records: 146 carry a `nodes` array, 56 more name no node but still reach
work under their goal, and 12 can use no path at all.

That is the shape of the problem. Both working paths reach an agent who is
already about to touch the file a lesson names, or who is working under the goal
it was learned under. Neither reaches an agent who is *asking a question* —
which is what search is for.

## Decision

Knowledge gains semantic matching, and it lives **behind the existing
`active_knowledge` call**.

`active_knowledge` is already the read surface, and already has a `contains`
substring filter. This is an upgrade to a call agents make today, not a new
place to look.

Two conditions bind the decision:

1. **No separate search verb.** Semantic matching must not arrive as a
   `search_knowledge` beside `active_knowledge`. A second surface recreates the
   very failure this ADR exists to close — a correct answer written where nobody
   reads.
2. **Degrade honestly.** When no embedder is reachable, fall back to substring
   matching and *say so in the response*. Silently returning fewer results makes
   "it searched and found nothing" indistinguishable from "it could not search",
   and the second is a defect wearing the first's clothes.

## Consequences

- Lodestar gains an embedding index for knowledge. Embeddings are produced over
  HTTP against an OpenAI-compatible endpoint, and Lodestar already depends on
  `ureq`, so this adds no new dependency.
- There is now a second vector store to keep honest, and a second thing that
  degrades when no embedder is reachable. That cost is accepted; condition 2 is
  what keeps the degradation visible rather than silent.
- The distinction between the two planes is unchanged. Nothing about this
  couples Lodestar to MindLeak.
- ADR-0053 decision 4 is delivered for `record_knowledge` by a different
  mechanism than it implied — an index inside the Intent Plane rather than a
  MindLeak node. The rejected alternatives explain why.

## Rejected alternatives

**Have `learned` also record a MindLeak node.** Infeasible as written, and the
constraint is mechanical rather than a matter of taste: `mindleak-core` is a
**dev-dependency only** of `lodestar-core`. Lodestar's runtime cannot write a
MindLeak node or reach its embedder, and that decoupling is deliberate under
ADR-0004. Taking this option means promoting the dependency and coupling the
Intent Plane to the Memory Plane.

**Have the client call `record_architectural_decision` after completing.** This
costs no coupling at all and has a working precedent, which makes it the
strongest rejected option. It loses because it writes durable conclusions into
the decaying episodic plane, and splits knowledge across two stores by accident
of which verb an agent remembered to call — which is how records became
unreachable in the first place.

**Add a `search_knowledge` verb.** Rejected by condition 1 above. Every
neighbouring defect in this repository has been a correct answer written where
nobody reads; a new search verb would have been one more.

**Narrow ADR-0053 decision 4 to say the two stores are deliberately different.**
Honest, and cheaper than any implementation, but it accepts that the most-used
write path stays unsearchable. The measurement above says the population that
can use no path at all is small (12 of 214) and no longer growing — but "small
and static" is an argument for fixing it once, not for declaring it correct.

## An honest weakening of the case

Lodestar knowledge decays too, on `decay::KNOWLEDGE_DEFAULT_HALF_LIFE_HOURS`,
with `reconfirm` to revalidate. The split between the planes is *slower
forgetting*, not durable-versus-decaying. This is recorded so the next reader is
not misled: the case for this decision rests on a single authoritative store and
an existing read surface, not on permanence.
