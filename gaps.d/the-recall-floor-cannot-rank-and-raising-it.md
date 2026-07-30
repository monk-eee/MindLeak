- **The recall floor cannot rank, and raising it makes recall worse — MEASURED,
  do not "fix" it.** The obvious response to `recall` returning a plausible
  stranger is to raise `MINDLEAK_RECALL_FLOOR`. Measured against this
  repository's own index over six real questions, that is backwards. Recorded
  conclusions scored **0.553–0.790**; structural nodes matched on shared
  vocabulary scored **0.527–0.667**. The ranges *overlap*, so any threshold high
  enough to exclude the worst stranger (0.667) also excludes real conclusions
  (0.553). One query returned the right conclusion at 0.651 and an unrelated
  `merge_import` symbol at 0.626 — a 0.025 gap that no global constant can
  separate.
  What does work is rank: a conclusion was the **top hit in six of six**
  queries. So the floor's job is to answer "is there anything here at all",
  which it does (ADR-0053), and it is not a relevance knob. If recall's
  precision needs improving, the lever is the ranking or the embedding model,
  not the threshold. Reproduce with a `recall` sweep and compare the score
  ranges before changing the default.

  **The ranking lever has since been taken; the floor advice above stands
  unchanged.** ADR-0075 stopped recall ordering by raw cosine: similarity is now
  weighted by node kind, so a recorded conclusion outranks a symbol that merely
  shares a word, which is exactly the 0.651-versus-0.626 case above that no
  constant could separate. The same change added a per-query distinctiveness cut
  for the reason this fragment gives — an absolute number cannot judge a score
  whose baseline moves with the query. Neither touched
  `MINDLEAK_RECALL_FLOOR`, and its default is deliberately unchanged. This is
  recorded so the next reader does not re-derive the measurement or re-implement
  the fix; what remains open is the *other* lever, the embedding model, which
  nothing here has tested.

  **Measured afterwards, and the overlap repeated itself one level up.** On the
  live 19,317-node index the ranking change did what it claimed — hits naming a
  node the graph no longer holds fell from 24 of 50 to 0 of 49, and recorded
  conclusions rose from 14% of what the caller is handed to 96%. But the
  distinctiveness cut does **not** let recall reject a question it has no answer
  for: top-hit distance above the field is 3.11–3.90 standard deviations for
  nonsense controls against 3.71–6.21 for real questions, so those bands overlap
  by 0.19σ exactly as the score ranges above do. A threshold in σ is still a
  global constant; moving from cosine to σ changed the units, not the shape of
  the problem. So the warning in this fragment generalises further than it was
  written: **the lever for "is there an answer here at all" is not a constant in
  any unit**, and nothing has yet found one that works. Numbers and harness in
  [EVALUATION.md](../docs/EVALUATION.md).
