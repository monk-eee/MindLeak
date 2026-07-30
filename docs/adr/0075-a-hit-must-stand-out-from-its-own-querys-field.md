# ADR-0075: A hit must stand out from its own query's field

- Status: Proposed
- Date: 2026-07-30
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
  not reach with an absolute threshold alone.
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
