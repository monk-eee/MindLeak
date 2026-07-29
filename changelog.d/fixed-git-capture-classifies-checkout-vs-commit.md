- **Passive Git capture now distinguishes checking out work from authoring it.**
  The sensor inferred intent from ancestry: if the new HEAD named the previous
  HEAD as a parent, it was ingested. That misattributed a checkout to a
  descendant branch as a commit by whoever viewed it, while event ordering
  could lose an explicit non-linear commit such as amend behind an in-flight
  state refresh.
  The sensor now tracks branch name as well as commit and consumes the built-in
  Git API's explicit `onDidCheckout` and `onDidCommit` events. A checkout is
  remembered without attribution; the next real commit on that branch is
  captured. An explicit commit upgrades a state refresh already in flight, so a
  non-linear commit is not dropped as a duplicate. A state-only non-linear move
  remains un-attributed and becomes the next baseline, because it may be reset,
  rebase, or checkout and attaching it would be worse than a visible gap.
  Eight focused tests cover descendant checkout, first commit after checkout,
  coalesced branch creation and commit, amend/event ordering, terminal-style
  linear advance, and state-only non-linear movement. The new tests fail four
  of eight against the previous implementation and pass eight of eight with the
  fix.
