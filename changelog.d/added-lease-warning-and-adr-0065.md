- **A lease about to die, or already dead, says so.** A lapse was silent until
  `complete_task`, which is far too late: closing a lapsed claim means
  re-claiming it, re-claiming records the lapse, and conformance then refuses to
  certify across the hole (ADR-0048), so the only warning arrived after the cost
  had become unrecoverable. Twenty-nine claims on this repository are stuck
  behind exactly that. `complete_task` now reports a claim within ninety seconds
  of expiry — the default lease is five minutes and `cargo test --all` alone can
  outlast it — and separately reports one that has already lapsed, with
  different advice, because renewing cannot repair a window that already has a
  hole in it. A comfortable lease says nothing at all: a warning on every call
  is a warning nobody reads.
- **ADR-0065 proposes that completion belongs at the publication boundary.**
  Everything in this project that relies on remembering has failed — ADR-0046
  measured zero adoption for a capability needing its own call — and everything
  hung off an action already being taken has held, from the publication ledger
  to the delivery queue. Completion is the last obligation still waiting to be
  remembered, and the cost of forgetting it is unrecoverable rather than merely
  untidy. `Proposed`: it gives publishing a second meaning, which is a real cost
  and deserves argument.
