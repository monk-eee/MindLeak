- **The board doctor no longer calls a deliberately parked task an ailment.**
  `blocked_without_gate` fired on every task blocked with no predecessor,
  including ones blocked with a stated reason. Because `blocked_by` is a
  one-to-one handoff (ADR-0015), a recorded reason is the only way to express
  "waiting on something that is not a single predecessor task" — so the doctor
  was reporting correct practice as a fault, forever, on a healthy board. It
  now exempts a task whose most recent `blocked` event stated a reason, and
  reports it again if a later block erases that explanation.
