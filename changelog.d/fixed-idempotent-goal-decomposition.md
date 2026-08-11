- **Decomposing a goal twice no longer produces the work twice.** `decompose_goal`
  created a task for every draft on every run, so re-running it left several
  identical tasks under one goal — measured here as three or four identical
  `Implement: ADR-NNNN` seeds for each of eight ADRs, from which two sessions
  independently claimed different seeds for ADR-0090 and built it twice. A draft
  whose exact title already names live work under the goal now resolves to that
  task, and `DecomposedTask` reports `reused` so a caller can tell the two apart.
  Underneath, "is this a duplicate?" was answered by the derived task id
  colliding, and that id hashes the creation second — so an identical title was
  refused when both creations landed in the same second and allowed a second
  later. The refusal now tests for live work of the same title at any distance in
  time, and the id disambiguates itself when a retired row already holds it.
  Terminal work does not suppress a draft, and `create_task` is unchanged: under
  ADR-0015 a person asking for a second task against one goal is often right,
  whereas a generator re-emitting a draft it already emitted has decided nothing.
