- **The pre-flight now answers the whole pre-flight question.**
  `check_overlap` already takes the paths and symbols an agent is about to
  touch, and the before-you-write checklist already mandates it — but it
  reported only which other agents were there. On a file nobody else was
  touching it returned an empty list, which reads as all-clear, while the graph
  already held that file's commit rationale and any execution that had failed
  on it. Learning that needed `get_impact_radius`, a second call at the moment
  attention has already moved on.
  Measured over this repository's lifetime telemetry: **8,109 ingests against
  66 reads at decision time** — `recall` 49, `graph_multi_hop_query` 10,
  `working_set` 4, `get_impact_radius` 3. Roughly 123 writes per read, plus
  32,980 dashboard polls (`graph_stats` alone has spent 57 minutes of compute
  answering "how many nodes are there"). The retrieval benchmarks were never
  wrong — `docs/EVALUATION.md` measures mean F1 0.77 against 0.44 for a vector
  arm — they answer *if you ask, is the answer good*, and never *does anyone
  ask*.
  So the answer now rides on the question agents already ask. `check_overlap`
  returns `impact` (dependents, previously failing executions, related
  intents), `unknown` (ids the graph has never seen), and `requested` alongside
  the existing `footprints`. No new tool: adding a sixth retrieval tool beside
  five that are already unused would repeat the failure, not fix it.
  `unknown` is reported separately from an empty `impact` on purpose. "The
  graph has never seen this file" and "nothing depends on this file" are
  different facts, and a caller that cannot tell them apart reads silence as
  reassurance.
  Deterministic and zero-token throughout; the ingest and query hot paths are
  untouched. See ADR-0066.
