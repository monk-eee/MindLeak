- **Publishing now records its own evidence, so work published through the gate
  can be certified (ADR-0009).** `canonical-push` already refused to publish
  without a live Lodestar claim, but wrote nothing to the Memory Plane — so
  `evidence_for` returned an empty bundle for work that had just been validated
  and pushed, `check_conformance` answered `needs_human` on
  *"evidence contains no provenance-bearing mutation"*, and `complete_task`
  refused. Measured on this repository, **18 of 21 human-blocked tasks stopped
  for exactly that reason**: one defect wearing eighteen hats, each one costing
  a human decision that had nothing to decide. The push is the right place to
  record — it is where a commit stops being a draft and becomes a fact about the
  world, and it is already the one path where the ledger is not optional. It
  ingests the published sha, subject, and changed files under the same session
  that holds the claim, after the push and never before. An unreachable graph
  warns and does not fail the push: the commit is already on the remote by then,
  and turning a missing record into a failed publication would trade one problem
  for a worse one.
