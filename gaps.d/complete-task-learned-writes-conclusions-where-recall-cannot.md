- **`complete_task(learned:)` writes conclusions where `recall` cannot see them
  — OPEN, and it is half of ADR-0053 undelivered.** ADR-0053 decision 4 says
  *"`record_knowledge` and `record_architectural_decision` embed the node they
  write"*. Only the second was implemented — by me, in the same change that
  quoted the decision. `record_architectural_decision` writes a MindLeak intent
  node and embeds it, so it is recallable immediately. `record_knowledge` — what
  `complete_task(learned:)` actually calls — writes to the Lodestar `knowledge`
  table, which has no embedder and no vector index, and `recall` searches
  MindLeak.
  Measured: 12 knowledge entries exist, and querying `recall` with the **exact
  text** of one returns unrelated execution nodes at 0.666 and 0.650, and the
  conclusion itself not at all.
  Impact: the write path most likely to be used is the one that rides on
  finishing work, and it is precisely the one whose output retrieval cannot
  reach. Real conclusions from several agents are sitting there — including
  *"PowerShell 5.1 reports exit code 1 when a native command writes anything to
  stderr"* — reachable only by listing `active_knowledge` in full, which does
  not scale and which nobody does.
  The fix is a design question rather than a patch: either Lodestar gains an
  embedding index for knowledge, or `learned` also records a MindLeak node, or
  the two stores are deliberately different things and ADR-0053 decision 4
  should be narrowed to say so. Recorded rather than guessed at.
