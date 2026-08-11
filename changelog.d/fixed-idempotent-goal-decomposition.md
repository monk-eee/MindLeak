- **Decomposing a goal twice no longer produces the work twice.** `decompose_goal`
  created a task for every draft on every run, so re-running it left several
  identical tasks under one goal — measured here as three or four identical
  `Implement: ADR-NNNN` seeds for each of eight ADRs, from which two sessions
  independently claimed different seeds for ADR-0090 and built it twice. A draft
  whose exact title already names live work under the goal now resolves to that
  task, and `DecomposedTask` reports `reused` so a caller can tell the two apart.
  Underneath, task IDs now add a deterministic discriminator when the base ID is
  occupied, so two deliberate tasks created in the same second receive distinct
  identities instead of colliding.
  Terminal work does not suppress a draft, and `create_task` remains deliberately
  permissive: under ADR-0015 a person asking for a second task against one goal is
  often right, whereas a generator re-emitting a draft it already emitted has
  decided nothing.
