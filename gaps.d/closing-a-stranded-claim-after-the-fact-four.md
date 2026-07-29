- **Closing a stranded claim after the fact: four traps, all hit in one
  sitting — OPEN.** Most stranded claims are work that already shipped and was
  never closed, so reaching for the receipt afterwards is a natural move. It is
  also full of holes, and two live tasks were transitioned to `in_review`
  learning them.
  1. **`check_conformance` is not a dry run.** It records an audit and can
     transition the task. A bundle that turns out wrong does not merely fail —
     it moves the task, and re-claiming then fails with
     `status in_review does not accept a claim right now`. Inspect the evidence
     bundle *before* submitting it; `evidence_for` is the read-only part.
  2. **The whole claim window is far too wide.** A claim that lapsed eighteen
     hours ago has had a day of unrelated commits land inside it. Ask for that
     span and the bundle sweeps them all in, and conformance correctly reports
     `drift` because the evidence covers governed code no covering task serves.
     Bound the window to the commit, not to the claim.
  3. **`ingest_commit` takes a `timestamp`, and you must pass it.** It defaults
     to now, so a historical commit is recorded as having happened today and no
     truthful window will ever contain it. Worse, the node is upserted: once
     created at the wrong time, ingesting again *with* the timestamp does not
     move `created_at`. The first careless call poisons that commit for good.
     `evidence_for` filters on the node's `created_at`, not on when the agent
     observed it.
  4. **The intent node is keyed by the sha string you pass.** Ingest `9ae2072`
     and the node is `intent:9ae2072`, not the resolved 40-character hash — so
     comparing against `git rev-parse` output reads a perfectly clean window as
     contaminated.

  And after all four are handled, **most of the list still cannot be closed.**
  A correctly bounded bundle for a documentation commit — one commit, two
  changed nodes, exactly right — still returns `needs_human`, because the
  evidence touches no code bound to the task's goal. Most stranded work is
  ADRs, Known-gaps entries and docs. Until ADR-0060 is implemented the list
  cannot be worked to completion, and attempting it converts `claimed` into
  `in_review` rather than into `done`.
