- **Completions that predate resolver attribution are accepted as historical
  (ADR-0069).** `resolve_task` validated the `human` argument and then discarded
  it, so for most of this project's life a human acceptance overriding a
  conformance verdict recorded that it happened but never by whom. The columns
  now exist and populate. Measured on the live board: **268 tasks, 147 `done`,
  17 carrying a resolver, 130 carrying none**, with the earliest recorded
  resolution at unix `1785285644`. Those 130 will not be reconstructed,
  annotated, or re-attested — the identity was never written anywhere, so there
  is nothing to recover, and re-accepting them now would manufacture attribution
  for judgements nobody can verify. `resolved_by IS NULL` on a completed task
  therefore means *predates attribution*, not *accepted by nobody*; a report
  rendering it as an absence of authority is wrong about what happened. The
  boundary is sharp from `1785285644` onward. Supersedes the earlier "57 of 101"
  figure, which measured a different cut (the verdict on the receipt rather than
  the presence of a resolver) on 28 July.
