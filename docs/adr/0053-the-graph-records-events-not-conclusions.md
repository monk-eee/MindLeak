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

Three separate causes were confirmed by reading the code, after a first pass at
this ADR asserted the wrong one:

**(a) There is nothing to retrieve.** Invariant 1 makes the write path
zero-token, so ingestion captures only what a machine emitted — executions,
diffs, AST symbols, artifacts. A conclusion is a sentence, and deterministic
pattern matching cannot manufacture one, so none is ever written.

**(b) `recall` has no similarity floor.** It is not full-text search. It is
cosine similarity over the optional embedding index
([ADR-0008](0008-semantic-recall-embedding-index.md)): `embed::recall` scores
*every* embedded node, sorts, and truncates to `limit`. There is no threshold
below which it declines to answer. It therefore always returns exactly `limit`
results, however unrelated. Demonstrated directly — the query
`zzzzqqq wibble flarp` returns `chore(vscode): register both local MCP planes`
at **0.54**, a *higher* score than any of the four real questions above scored.
Nonsense outranks a genuine query. Every one of those four "results" was the
nearest vector in an unrelated cloud, presented with a number that reads like
confidence.

**(c) A recorded conclusion is invisible until an offline pass runs.** Embeddings
are produced only by `index_nodes`. Recording ADR-0053's own decision through
`record_architectural_decision` created `intent:8ac3a2338d52` — and `recall`
for `ADR-0053 graph records events not conclusions` returned **`[]`**, because
the node it had just created had no vector yet. The verb this ADR is about
writes knowledge that cannot be read back until someone remembers to reindex.

Meanwhile all four lessons *were* durably recorded and *were* acted on — in
prose, in flat files: ADRs, `CHANGELOG.md`, the Known gaps section of
`DEVELOPERS.md`, and the agent's own markdown memory notes. The agent consulted
those notes and changed its behaviour because of them. It never once called
`recall` for a lesson, and got nothing when it finally did.

That comparison has to be stated plainly, because it is the uncomfortable part:
**for recalling knowledge, a flat markdown file outperformed a 9,572-edge
decay-weighted graph.** Not by a little. The file contained sentences, returned
them exactly, and needed no index pass. And it never invented an answer, which
is the failure that matters most: an empty `grep` is honestly empty, whereas
`recall` cannot say "I do not know".

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

3. **`recall` gets a similarity floor and is allowed to return nothing.** Below
   a configurable threshold, `embed::recall` returns no rows rather than its
   nearest neighbours. An honest empty answer is strictly more useful than a
   confident wrong one: the caller can then fall back to `multi_hop_query`,
   `graph_snapshot`, or reading the repository, whereas today it is handed
   `cargo check` and no way to tell that is not an answer. This is the single
   highest-value item here — it is worth more than decision 2, because without
   it every recorded conclusion still arrives buried under five plausible
   strangers.

4. **Recording a node indexes it.** A conclusion that cannot be read back until
   someone remembers to run `index_nodes` is not recorded, it is queued.
   `record_knowledge` and `record_architectural_decision` embed the node they
   write, and degrade to writing it unembedded — with that fact visible — when
   no embedding server is reachable. This does not breach invariant 1: those two
   verbs are *already* the explicit, human-or-agent-supplied path, not the
   deterministic ingest hot path, and `ingest_*` is untouched.

5. **Conclusions decay on their own clock.** Invariant 2 holds — nothing here
   disables decay. But "PowerShell reports exit 1 on any stderr write" does not
   go stale on the same half-life as a single `cargo check`. Knowledge gets a
   long half-life, and `reconfirm_knowledge` resets it on use. A lesson nobody
   reconfirms fades out, which is decay doing precisely the job it was designed
   for.

6. **This does not replace per-agent scratch notes, and must not try to.** Local
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
- **Decision 3 will make `recall` return fewer results, and sometimes none.**
  That will read as a regression to anyone who mistook the old output for
  answers. It is the opposite, but the threshold has to be tuned against real
  queries and defended, not picked. It is a behavioural change to the one verb
  every consumer depends on and needs its own tests.
- Decision 4 makes two verbs depend on a reachable embedding server where they
  previously did not. They must degrade rather than fail — invariant 4 says the
  deterministic path never depends on a model, and recording a conclusion must
  not become impossible because Ollama is down.
- **The measurement in this ADR was wrong on its first pass, and that is worth
  keeping.** It asserted `recall` was FTS5 ranked by decayed weight. It is
  cosine similarity over an embedding index. The observation was right and the
  cause was wrong, which is precisely the failure mode a graph full of events
  and empty of conclusions produces: plenty of evidence, no one who wrote down
  what it meant.
- The honest scope of the finding: the *coordination and evidence* half of this
  system — claims, leases, `evidence_for`, conformance refusing to certify work
  it cannot bound — was not in question and did its job all session. It is the
  *memory* half that lost to a text file, and only because it was never given
  anything to remember.

## Not implemented in this build

Nothing here is built. The measurement above is reproducible against any
populated graph, and this ADR records the finding so the next session does not
have to spend eight hours rediscovering that the graph never learned anything.
