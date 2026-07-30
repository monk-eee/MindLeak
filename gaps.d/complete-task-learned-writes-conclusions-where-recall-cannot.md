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
  reach.

  **Re-measured 2026-07-30, and the reachability half of this entry was
  incomplete.** It said such conclusions are reachable only by listing
  `active_knowledge` in full, which nobody does. There is a second delivery
  path and it works: the conformance advisory matches recorded knowledge on
  referenced nodes and hands it to the agent at completion. Verified end to
  end — `knowledge:77be310382e0` arrived as one of five advisories on an
  aligned completion (check 350 on `task:9aac89a91b00`'s predecessor), having
  been unreachable an hour earlier.

  That does not answer the design question; it sharpens it. The working path is
  **node-matched, not semantic**. It reaches an agent who is already about to
  touch the file the lesson names, and it reaches nobody who is asking a
  question — which is what `recall` is for, and why ADR-0053 decision 4 is still
  half delivered. It also has a measured cost of its own: 67 of 174 active
  records carry no `nodes` array, so they can use neither path. `record_knowledge`
  now reports `surfaces` when a record can never be read, and its schema states
  what evidence must carry, so the population stops growing; the existing 67
  need rescuing by hand, because nothing attaches nodes retrospectively.

  The fix is a design question rather than a patch: either Lodestar gains an
  embedding index for knowledge, or `learned` also records a MindLeak node, or
  the two stores are deliberately different things and ADR-0053 decision 4
  should be narrowed to say so. Recorded rather than guessed at.
