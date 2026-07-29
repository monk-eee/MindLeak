- **Human task-review attribution is checked but not persisted.** —
  `facade::executive::resolve_task` requires a non-empty human identity and
  refuses the worker recorded in the latest conformance evidence, but
  `store::coordination::resolve_in_review` records only the transition to
  `done`; the supplied reviewer and decision timestamp are not written to an
  append-only review record. — Medium audit impact: the ledger proves that a
  task required review and later became done, but cannot answer who accepted
  it. The Work view asks for identity only to enforce independent review and
  does not claim durable attribution. — **Left for later:** add a task-resolution
  audit record and expose it in task proof without rewriting conformance history.
