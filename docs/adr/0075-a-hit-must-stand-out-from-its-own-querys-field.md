# ADR-0075: A hit must stand out from its own query's field

- Status: Accepted
- Corrected: 2026-07-30, after measurement — see "Correction" at the foot. The
  decision stands; one claimed consequence did not survive contact with a real
  index and is restated there rather than left standing.
- Date: 2026-07-30
- Deciders: MindLeak maintainers
- Accepted: 2026-08-18 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Related: [ADR-0008](0008-semantic-recall-embedding-index.md) (the optional
  semantic recall index), [ADR-0053](0053-the-graph-records-events-not-conclusions.md)
  (the floor, and why saying nothing is an answer),
  [ADR-0002](0002-sqlite-decay-over-vector-llm.md) (decay graph over vector
  memory)

## Context

`recall` ranked candidates by cosine similarity alone and kept everything above
an absolute floor. Two defects were reported against that design, filed
separately, and both were read as "the floor needs tuning". They are one defect,
and the floor was never the cause.

**Cosine similarity is not comparable across queries.** Embedding spaces are
anisotropic: every text carries a baseline resemblance to every other text, and
the size of that baseline moves with the query. Measured 2026-07-27 against this
repository's own index (4,463 nodes, 9,572 active edges), the nonsense query
`zzzzqqq wibble flarp` scored **0.54** -- above the 0.5 default floor -- because
the whole field scores about that for any question at all. The floor was asked
to compare a number against a constant when the number has no fixed meaning.

**Raising the floor is measurably worse, not better.** Over six real questions,
recorded conclusions scored 0.553-0.790 while structural nodes matched on shared
vocabulary scored 0.527-0.667. The ranges overlap. Every constant high enough to
exclude the worst stranger (0.667) also excludes real conclusions (0.553). One
query returned the right conclusion at 0.651 and an unrelated `merge_import`
symbol at 0.626 -- a 0.025 gap that no global constant can separate.

**Ranking by cosine alone contradicted the goal the module serves.** The
governing objective states that this engine replaces "flat-log or vector-only
memory" and that "embeddings may only seed graph traversal". A recall that
orders results purely on embedding distance is not seeding traversal; it asks
the embedding model to be the whole answer, and discards everything the graph
already knows about each candidate. The statement was describing the defect
before the defect was diagnosed.

The practical cost is that a caller handed a plausible stranger cannot tell it
is wrong, so the caller learns to stop asking.

## Decision

Recall keeps its floor and gains two rules, both computed from data it already
has.

1. **A candidate must stand out from its own query's field.** One pass over the
   scores already computed yields the field's mean and spread; a candidate must
   clear one standard deviation above the mean to count as an answer rather than
   as background. Distinctiveness is a per-query question, so it is asked per
   query. A nonsense question lifts the whole field uniformly, nothing stands
   above it, and recall answers nothing.

   Two boundaries keep this honest. A field of fewer than eight candidates has
   no shape to reason about, so the floor alone decides and a young index is
   never silenced by statistics it cannot support. A field with no spread at all
   has no outliers by definition, so nothing is returned.

2. **The graph ranks, not the embedding model.** Similarity is weighted by a
   prior on node kind before ordering: a recorded conclusion or decision
   (`intent`) outranks an artifact, which outranks a symbol or execution, which
   outranks a package, which outranks an agent. A conclusion is what a question
   is usually for; a function name that shares one word with the question is
   not.

   This is deliberately a tie-breaker rather than an override. The spread is
   narrow enough that a genuinely closer symbol still outranks a barely-related
   intent, so structural questions such as "which function parses imports" keep
   working. It decides the near-ties, which is exactly where the measured
   overlap lives.

The reported score remains the raw cosine, so a caller sees the similarity that
was measured rather than an internal ranking composite.

## Alternatives rejected

**Raise `MINDLEAK_RECALL_FLOOR`.** The obvious response, and the measurement
refutes it: the conclusion and structural-noise score ranges overlap, so no
constant separates them, and every value that removes the worst stranger also
removes real answers. This is recorded so the next reader does not re-derive it.

**Replace the embedding model.** A better model would raise absolute scores but
not make them comparable across queries; anisotropy is a property of the space,
not of one model's quality. It also could not supply what the kind prior
supplies, because the distinction between a conclusion and a symbol lives in the
graph rather than in the text. Worth revisiting for ranking quality, but it does
not address either defect.

**Normalise the vectors at index time.** This shifts the baseline without
removing the query-dependence of the baseline, so a constant floor remains
uncomparable across queries.

## Consequences

- A question with no answer in the index is answered with silence rather than
  with a confident stranger, which is the outcome ADR-0053 intended and could
  not reach with an absolute threshold alone. **This did not survive
  measurement — see the Correction below.**
- Recall's precision now improves by improving the ranking signal -- the graph --
  rather than by tuning a constant. The floor is not a relevance knob and should
  not be used as one.
- An embedding may outlive the node it described, so candidate lookup tolerates a
  missing node and ranks it as ordinary structure rather than failing the query.
- The cost is one additional pass over scores already in memory and one join
  against a table already open. No LLM call joins the read path, and the
  zero-token write path is untouched.
- The kind prior is a policy embedded in code. If recall is later asked to serve
  a caller whose questions are mostly structural, that prior is the thing to
  revisit, not the floor.

## Correction (2026-07-30, measured)

This decision was accepted on deterministic unit tests whose fields were
synthetic and uniform. Measured afterwards against this repository's real index
(19,317 embedded nodes), two of its claims held and one did not. It is corrected
here rather than left standing because the record is still Proposed, and a
decision record that overstates its own result is worse than none.

**Held.** Hits naming a node the graph no longer holds fell from 24 of 50 to 0
of 49, and recorded conclusions rose from 14% of what the caller is handed to
96%. The ranking change does what it claimed.

**Did not hold: "a question with no answer is answered with silence."** On a
real index a nonsense query still returns hits. Distance of the top hit above
its own field is 3.11–3.90 standard deviations for nonsense controls and
3.71–6.21 for real questions: the bands overlap by 0.19σ, so no single value of
`DISTINCTIVE_SIGMA` rejects nonsense while keeping every real answer. The
shipped 1σ cut sits far below both bands and trims almost nothing.

The error was in the reasoning quoted above — that "a nonsense question lifts
the whole field uniformly". It does so only in a uniform field, which is what
the fixture built and what a 19,000-node index is not. In a diverse field even
nonsense has relative outliers.

The constant is deliberately not tuned in response: three nonsense samples
separated by a negative margin is exactly the global constant this ADR's own
Context rejects, one level up. What the measurement establishes is narrower than
what was claimed — distinctiveness improves *ranking*, but it is the wrong shape
for deciding *whether a query has any answer at all*, and that question is open.
Recorded in [EVALUATION.md](../EVALUATION.md) with a reproducible harness.

## Resolution (2026-07-31, measured)

The open abstention question is resolved by grounding the semantic result in the evidence it returns, not by moving another similarity constant. After cosine and graph-kind ranking produce candidates, `recall` tokenises the informative terms in the query and the returned node text. Each query term is weighted by inverse document frequency over the graph's FTS corpus, $\ln((N+1)/(df+1))+1$, and the result is served only when its nodes support at least half of that information. Generic words therefore cannot outweigh an unsupported repository-specific term. Queries with fewer than three informative terms keep the previous fuzzy-recall contract because there is too little language to judge support honestly.

This is deliberately evidence-local. A corpus-wide lexical check failed in measurement because the repository contains its own evaluation text, including `zzzzqqq wibble flarp`; the query existed somewhere, while the semantic hits returned for it were unrelated. Grounding against the candidate nodes asks the useful question: can this answer explain why it matched?

The expanded live-index evaluation used three gibberish controls, four coherent natural-language questions absent from this repository, and seven questions labelled real. Before the gate every negative control returned five hits; afterwards **7 of 7 abstained**. Five real questions had relevant returned evidence and all **5 of 5 remained answered**. The PowerShell and stale-server questions had previously been counted as successful solely because they returned non-empty lists; inspecting their labels showed unrelated report scripts and merge commits, so the corrected relevance check records them as unanswered and the grounding gate now abstains. All 25 served hits are current graph nodes and intents.

The precision tradeoff is explicit: a true paraphrase with no informative term in its candidate evidence can now return nothing. That is preferable to a plausible stranger, and the caller can rephrase or fall back to FTS and graph traversal. The gate adds no LLM call, no additional embedding request, and no change to `MINDLEAK_RECALL_FLOOR` or `DISTINCTIVE_SIGMA`. Reproduce the result with `scripts/evaluate-recall.mjs`; the machine-readable artifact is [`2026-07-31-recall-grounding.json`](../../benchmarks/results/2026-07-31-recall-grounding.json).
