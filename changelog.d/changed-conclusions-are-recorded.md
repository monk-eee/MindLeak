- **Recall can now say "I don't know", and conclusions get recorded (ADR-0053).**
  Measured on this repository's own index: asking `recall` the five questions an
  agent actually had during a day's work returned code locations, never
  experience — "canonical-push auto-merge armed refuses" came back with
  `merge_import`, a symbol matched on the word "merge", with nothing to mark it
  as noise. A caller handed a plausible stranger cannot tell it is wrong, so it
  stops asking, and that is the whole adoption problem. `recall` now applies a
  cosine floor (`MINDLEAK_RECALL_FLOOR`, default 0.5) and returns **nothing**
  when nothing clears it. An honest empty answer is usable: fall back to
  `multi_hop_query`, `graph_snapshot`, or the repository.
  The other half is that there was nothing worth recalling. A 500-node sample of
  the graph held 196 executions, 159 symbols, 120 artifacts — and no conclusions,
  because nothing ever asked for one. `complete_task` now takes `learned` and
  records it as durable knowledge at the moment the agent holds it; omitting it
  never blocks completion, because most tasks teach nothing and a gate would only
  produce a column of `n/a`, but the response names the omission so the gap is
  measurable instead of invisible. `record_architectural_decision` embeds the
  node it writes, so a recorded conclusion is recallable immediately rather than
  queued until someone remembers to run `index_nodes`; when no embedding server
  is reachable the node is still written and `embedded: false` says so. The
  zero-token write path is untouched: a conclusion is supplied, never inferred
  from an execution log.
