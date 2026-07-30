- **`recall` ranks with the graph instead of by cosine alone, and can tell
  background from an answer.** Two measured defects, one cause: ranking asked
  the embedding model to be the whole answer, which is the vector-only memory
  the engine exists to replace.
  - _A plausible stranger read exactly like a hit._ Cosine similarity is not
    comparable across queries — embedding spaces are anisotropic, so every text
    carries a baseline resemblance to every other text. Measured against this
    repository's own index, the nonsense query `zzzzqqq wibble flarp` scored
    0.54, above the 0.5 default floor, because the whole field scores about
    that for any question at all. Raising the floor is measurably worse, not
    better: recorded conclusions scored 0.553-0.790 and structural nodes
    matched on shared vocabulary scored 0.527-0.667, so every constant that
    excludes the worst stranger also excludes real conclusions. Recall now asks
    whether a candidate stands out from its own query's field, so a question
    with no answer in the index is answered with silence.
  - _A function name outranked a recorded conclusion._ One query returned the
    right conclusion at 0.651 and an unrelated `merge_import` symbol at 0.626,
    a gap no threshold can separate. The graph can separate it: one is a
    recorded conclusion, the other is a symbol sharing a word. Ranking now
    weights similarity by node kind, as the governing goal requires when it
    says embeddings may only seed graph traversal. The weighting is a
    tie-breaker rather than an override, so a genuinely closer symbol still
    wins and structural questions keep working.

  The similarity floor keeps its original job (ADR-0053) and its default is
  unchanged. A field too small to have a shape is still judged by the floor
  alone, so a young index is never silenced by statistics it cannot support.
  The reported score stays the raw cosine rather than the internal ranking
  composite. No LLM call joins the read path and the zero-token write path is
  untouched. Recorded as ADR-0075, including the two rejected alternatives
  (raise the floor; change the embedding model) and the measurement that
  rejects each, so the next reader does not re-derive them.
