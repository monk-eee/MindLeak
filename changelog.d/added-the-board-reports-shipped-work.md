- **The board now reports the claims it cannot close.** Work that shipped and
  never closed stays on the board indefinitely, and a board that understates what
  is finished is expensive in a way an overstated one is not: `next_task` offers
  work that already exists and an agent rebuilds it. Observed repeatedly on this
  repository — a task was offered whose branch was sitting in an open pull
  request, and four separate open tasks turned out to be already delivered, each
  costing a fresh investigation to discover.
  `make board-health` now names any non-terminal task whose recorded branch has
  merged into `main`, with the merge commit, so a person can check it in seconds.
  Branches are read from merge subjects rather than `git branch --merged`,
  because a branch is usually deleted the moment it merges — the ref is gone
  while the history proving it landed is not.
  It reports and never closes. Completing one of these would manufacture a
  receipt for work the script did not witness, which ADR-0009 refuses.
  The count distinguishes **`unknown` from `0`**, which matters more than the
  feature: a task claimed before the branch column existed records none, and a
  server built before it does not return the column at all. Both produce an empty
  result that reads as "nothing shipped unclosed" while actually meaning "nothing
  to check against". The first live run produced exactly that false zero. A bare
  count there would have been the same falsely-reassuring signal this report
  exists to remove.
