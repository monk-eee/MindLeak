- **`DEVELOPERS.md` records what closing a stranded claim after the fact
  actually costs.** Most stranded claims are work that shipped and was never
  closed, so reconstructing the receipt is a natural move — and it has four
  traps that were all hit in one sitting, transitioning two live tasks to
  `in_review` in the process. `check_conformance` is not a dry run: it records
  an audit and moves the task, after which re-claiming fails. The whole claim
  window is too wide and produces `drift` from unrelated commits. `ingest_commit`
  defaults its `timestamp` to now, and because the node is upserted, one
  careless call fixes that commit at the wrong time permanently. The intent node
  is keyed by the sha string as passed, so comparing against `git rev-parse`
  reads a clean window as contaminated. And after all four are handled, a
  correctly bounded bundle for a documentation commit still returns
  `needs_human` — so until ADR-0060 is implemented the list cannot be worked to
  completion at all.
