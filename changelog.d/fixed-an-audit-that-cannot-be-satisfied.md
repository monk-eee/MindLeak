- **The merged-branch audit failed on work that had fully landed.** It compared
  ancestry, so a squash or rebase merge — which lands every line under a new
  commit id — was indistinguishable from a branch whose commits never merged at
  all. It then reported the work as lost and instructed the reader to open a
  follow-up pull request for changes already on `main`, which is not something
  anyone can do. That is the failure mode worth naming: an audit with no green
  move available gets switched off, and switching this one off would take the
  check that catches genuinely lost work with it. It also cost real time before
  it was fixed — PR #205's work was recorded in durable knowledge as lost, and a
  follow-up to restore 245 lines that were already on `main` was queued against
  the one file three agents were editing. `git merge-base --is-ancestor` answers
  a question about commit identity, not about whether the work arrived. The
  audit now uses `git cherry`, which compares patches: a commit whose changes
  never reached the base still fails the build, while one that landed under a
  rewritten id is reported as landed-but-rewritten and does not. Merge commits
  are in neither list, since merging the base into a branch carries no work of
  its own and reporting it as lost was noise obscuring the one real finding.
  Nothing weakens: the report says plainly that a squash or rebase merge
  destroyed a commit identity AGENTS.md asks to keep, and points at the
  repository setting that prevents it, because the only durable fix is at the
  merge button rather than in an audit that runs afterwards. The module's own
  comment claimed its helpers were covered by tests and no test file existed;
  there is one now.
