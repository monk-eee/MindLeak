- **Semantic recall was answering from a three-day-old snapshot, and nothing
  said so (ADR-0008).** The embedding index is populated by `index_nodes`, an
  explicit offline pass — and nothing ever called it. The maintenance worker ran
  `autonomous_prune` **423 times** and the index pass **once, ever**, three days
  earlier, by hand. Measured on this repository: **5,443 nodes carried no vector
  at all**, and every `recall` result predated that single run. Asked *"ingest a
  git commit as an intent node linked to changed files"*, recall returned nine
  shell invocations of `git commit` and zero code symbols; after a refresh the
  top hit is `ingest_commit` in `ingest/git.rs` at **0.770**. The graph knew; the
  index could not see it.
  A stale index is worse than a missing one. Without an embedding server recall
  errors cleanly and you know where you stand; with a stale one it answers
  confidently from whatever the last manual pass happened to cover, and nothing
  in the result says how old that is. This is also why the recorded "recall floor
  cannot rank" measurement was misleading — it compared score ranges drawn
  entirely from stale nodes.
  The worker now runs the index pass on the prune's activity-independent
  cadence, in bounded batches, recording `autonomous_index` telemetry and
  degrading cleanly when no embedding server is reachable. Indexing is now its
  own switch, `MINDLEAK_AUTONOMOUS_INDEX` (default on, cadence
  `MINDLEAK_INDEX_INTERVAL_SECS`, batch `MINDLEAK_INDEX_BATCH`), separate from
  `MINDLEAK_AUTONOMOUS_CONSOLIDATION`: the two shared one default-off flag, which
  conflated cheap local embedding with expensive generation and is precisely why
  the pass never ran.
