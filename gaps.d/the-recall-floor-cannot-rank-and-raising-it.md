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
