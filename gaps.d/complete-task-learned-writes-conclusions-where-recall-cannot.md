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

  That does not answer the design question; it sharpens it. The working paths
  are **node-matched and goal-matched, not semantic**. They reach an agent who
  is already about to touch the file the lesson names, or who is working under
  the goal it was learned under; they reach nobody who is asking a question —
  which is what `recall` is for, and why ADR-0053 decision 4 is still half
  delivered. The measured cost here has shrunk and changed shape: of 214
  records, 146 carry a `nodes` array, 56 more name no node but still reach work
  under their goal, and **12 can use no path at all**. Only those 12 need
  rescuing by hand. The earlier figure — 67 of 174 with no nodes, described as
  reaching nobody — counted the node path alone, and was left standing after
  the goal path landed. `record_knowledge` reports `reach` at write time and
  its schema states what evidence must carry, so the population stops growing.

  The fix is a design question rather than a patch: either Lodestar gains an
  embedding index for knowledge, or `learned` also records a MindLeak node, or
  the two stores are deliberately different things and ADR-0053 decision 4
  should be narrowed to say so. Recorded rather than guessed at.

  **The constraint that decides between them, measured from the manifests.**
  `mindleak-core` is a **dev-dependency only** of `lodestar-core`, so Lodestar's
  runtime cannot write a MindLeak node or reach its embedder. That decoupling is
  deliberate under ADR-0004, not an oversight to patch around.
  So the second option is infeasible as written — having `learned` record a
  MindLeak node means promoting that dependency and coupling the Intent Plane to
  the Memory Plane. There is a variant this entry did not consider that costs no
  coupling at all: the **client** calls `record_architectural_decision` after
  completing, which is a convention rather than an architectural change, and has
  a working precedent in the one verb that is already recallable.
  The first option is cheaper than it reads. Embeddings are produced over HTTP
  against an OpenAI-compatible endpoint (`MINDLEAK_EMBED_URL`, default
  `http://localhost:11434/v1`) rather than by a linked-in model, and Lodestar
  already depends on `ureq` — so it needs no new dependency. Its real cost is a
  second vector store to keep honest, and a second thing that degrades when no
  embedder is reachable.
  Still not selected here: this narrows the choice to two viable shapes and
  leaves the judgement where it belongs.

  **Chosen by the maintainer, 30 Jul 2026: option 1, with a condition.** Recorded
  as a decision taken rather than one an agent drifted into — the entry sat
  deliberately unselected until asked.
  The basis is narrower than the option first reads. `active_knowledge` already
  exists as a read surface with a `contains` substring filter, so this is an
  **upgrade to a call agents already make**, not a new place to look. That
  distinction is the whole reason to prefer it: every neighbouring defect in
  this repository has been a correct answer written where nobody reads, and a
  new search verb would have been one more.
  *The condition.* Semantic matching stays **behind the existing call**. A
  separate `search_knowledge` beside `active_knowledge` recreates the failure
  this entry describes. And it must degrade honestly when no embedder is
  reachable — fall back to substring and say so in the response, because
  silently returning fewer results makes "it works" and "it ran and found
  nothing" indistinguishable.
  *Why not the client-side variant*, despite costing no coupling: writing
  conclusions into MindLeak puts durable intent into the decaying episodic
  plane, and splits knowledge across two stores by accident of which verb an
  agent remembered to call — which is exactly how 67 records became unreachable.
  *One honest weakening of that argument*, recorded so the next reader is not
  misled: Lodestar knowledge decays too, on
  `decay::KNOWLEDGE_DEFAULT_HALF_LIFE_HOURS` with `reconfirm` to revalidate. The
  split is slower forgetting, not durable-versus-decaying, and the case for
  option 1 rests on the single authoritative store and the existing read
  surface rather than on permanence.
