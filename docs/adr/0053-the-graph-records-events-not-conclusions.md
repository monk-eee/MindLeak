# ADR-0053: The graph records events, not conclusions

- Status: Proposed
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Related: [ADR-0002](0002-sqlite-decay-over-vector-llm.md) (SQLite decay over
  vector/LLM), [ADR-0008](0008-semantic-recall-embedding-index.md) (semantic
  recall embedding index), [ADR-0009](0009-evidence-backed-conformance.md)
  (evidence-backed conformance),
  [ADR-0046](0046-agents-talk-through-the-durable-thread.md) (agents talk
  through the durable thread)

## Context

MindLeak exists so an agent does not have to relearn a repository every session.
On 2026-07-27, after an eight-hour session against this repository, that claim
was tested directly rather than assumed.

The graph was not small: **4,463 nodes and 9,572 active edges**. The session had
produced eleven merged pull requests, three ADRs, and four expensive lessons —
each one costing real time to discover. Those four lessons were put to `recall`
verbatim:

| Query | Top results |
|---|---|
| `powershell exit code stderr NativeCommandError false failure` | `execution: cargo test -p lodestar-core` (0.61), `cargo check` ×3 |
| `lease_secs default 300 claim expired lapsed` | `symbol: renew_lease (fn)` (0.63) |
| `merge conflict CHANGELOG union gitattributes` | `symbol: merge_import (fn)` (0.58) |
| `no such column audience index migration order` | `symbol: previous_non_newline (fn)` (0.54) |

Four out of four returned noise. Not a weak answer — no answer, dressed as one.
`merge_import` is a lexical collision on the word "merge"; the cargo commands
share no meaningful token with the query at all.

The reason is not a ranking defect. `recall` searches FTS5 over node labels and
content, ranked by decayed weight ([ADR-0002](0002-sqlite-decay-over-vector-llm.md);
the optional embedding index of [ADR-0008](0008-semantic-recall-embedding-index.md)
is off by default and was not indexed). It returned command lines and symbol
names because **command lines and symbol names are the only things in there.**
That follows directly from invariant 1: the write path is zero-token, so
ingestion can capture only what a machine emitted — executions, diffs, AST
symbols, artifacts. A conclusion is a sentence. Deterministic pattern matching
cannot manufacture one, so none is ever written.

Meanwhile all four lessons *were* durably recorded and *were* acted on — in
prose, in flat files: ADRs, `CHANGELOG.md`, the Known gaps section of
`DEVELOPERS.md`, and the agent's own markdown memory notes. The agent consulted
those notes and changed its behaviour because of them. It never once called
`recall` for a lesson, and got nothing when it finally did.

That comparison has to be stated plainly, because it is the uncomfortable part:
**for recalling knowledge, a flat markdown file outperformed a 9,572-edge
decay-weighted graph.** Not by a little. The file contained sentences; the graph
contained events.

The capability is not missing. `record_knowledge` (Intent Plane) and
`record_architectural_decision` (Memory Plane) both exist and both write exactly
the kind of node that was wanted. In eight hours neither was called once. Nothing
in the working loop asks for one, nothing measures the omission, and an agent
that never reaches for a verb may as well not have it.

## Decision

**A conclusion is supplied, not extracted — and the loop must ask for it.**

1. **The zero-token write path is not the bug and does not change.** Invariant 1
   stands: deterministic ingestion never calls a model. This decision does not
   propose inferring lessons from executions. It proposes that whoever already
   holds the conclusion writes it down at the moment they hold it.

2. **Recording what was learned becomes part of finishing work, not an extra.**
   `complete_task` accepts what the agent learned and, when it is omitted,
   **reports the omission** rather than passing silently. Completion is not
   blocked: many tasks teach nothing, and a gate would only produce a column of
   `learned: n/a`. Making the gap visible is what turns it from invisible into
   measurable.

3. **Prose outranks events for a prose question.** A knowledge or decision node
   must rank above execution and symbol nodes when a query has no lexical anchor
   in code. Recording is worthless if retrieval buries one sentence under four
   thousand command lines — the failure above would survive the fix otherwise.

4. **Conclusions decay on their own clock.** Invariant 2 holds — nothing here
   disables decay. But "PowerShell reports exit 1 on any stderr write" does not
   go stale on the same half-life as a single `cargo check`. Knowledge gets a
   long half-life, and `reconfirm_knowledge` resets it on use. A lesson nobody
   reconfirms fades out, which is decay doing precisely the job it was designed
   for.

5. **This does not replace per-agent scratch notes, and must not try to.** Local
   markdown loaded into context at zero latency is a different artefact from
   fleet-shared, attributed, decaying knowledge. The former is one agent's
   working memory; the latter is what the next agent inherits. An agent that
   deletes its notes on the strength of this ADR has lost the thing that
   demonstrably worked.

## Consequences

- The graph starts carrying sentences a human would want to read back. That is a
  change in kind, not degree: today no query can be answered with "because…".
- **Agents will record low-value lessons.** This is the cost, and the mitigation
  is already built: `prune_knowledge` and `reconfirm_knowledge` mean an unused
  lesson decays out on its own. Notably this is the first case where decay is
  obviously right rather than contestable — nobody wants a graph of every
  half-truth an agent believed in July.
- Prompting at completion adds friction to every task, including the many that
  teach nothing. Decision 2 keeps that friction to a report rather than a gate
  for exactly that reason.
- Decision 3 changes `recall` ranking, which will move results for existing
  queries. That is intended, but it is a behavioural change to the one verb
  every consumer depends on and needs its own tests.
- The honest scope of the finding: the *coordination and evidence* half of this
  system — claims, leases, `evidence_for`, conformance refusing to certify work
  it cannot bound — was not in question and did its job all session. It is the
  *memory* half that lost to a text file, and only because it was never given
  anything to remember.

## Not implemented in this build

Nothing here is built. The measurement above is reproducible against any
populated graph, and this ADR records the finding so the next session does not
have to spend eight hours rediscovering that the graph never learned anything.
