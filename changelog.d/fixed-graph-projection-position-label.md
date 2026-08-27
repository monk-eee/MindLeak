- **The Context Graph page no longer calls the projection checkpoint the ledger
  position.** The header rendered `Ledger position <n>` while `n` was the
  projection's own stream position — a different quantity, because a projection
  only consumes `structural_fact` records and checkpoints at the last one it
  consumed. Verified against a repository whose ledger head is 2 and whose
  projection checkpoint is 1: the page displayed "Ledger position 1". The label
  now names the value it actually shows, matching `index.html` and
  `administration.html`, which already distinguish the two. The graph endpoint
  returns only the projection position, so this is a naming fix rather than a new
  API field.

  The mislabel was undetectable until the freshness fix established that the two
  numbers legitimately differ; while every surface conflated them, a wrong label
  and a right one rendered identically. The accompanying test asserts the correct
  wording *and* refuses the old wording, so it cannot quietly return.

  Also fixes the node and edge counts reading "Showing 1 nodes, 0 edges".
