- **A conflicted merge keeps the provenance of what it resolved.** Every merge
  commit was treated as noise on the grounds that its content already arrived on
  the branches it joins — true of a clean merge, and false of a conflicted one,
  where the resolution is genuinely authored. The cost was that reconcile-shaped
  work could not be certified at all: a reconcile's entire product *is* the merge
  commit, so the evidence window came back empty however much conflict
  resolution it contained, and `check_conformance` had nothing to judge.
  Git already draws the line in the right place. `git show --name-only` on a
  merge reports the combined diff — only files differing from *every* parent,
  which is exactly what the merge itself introduced. Measured across 25 merge
  commits in this repository, that set matched "differs from every parent" in
  25 of 25 cases and was empty for all 18 clean merges, so the parent count was
  never needed: an empty changed-file list already means the commit authored
  nothing. A clean merge is still skipped, one git call disappears from a hook
  that runs on every commit, and the claim about git's behaviour is now covered
  by a test against real git rather than a fake, because a clean merge wrongly
  ingested would attribute another agent's whole branch to whoever ran it.
