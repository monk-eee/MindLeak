- **The delivery queue no longer loses a minute on every merge.** Immediately
  after a merge GitHub recomputes mergeability and every queued pull request
  reads `UNKNOWN` for a few seconds. That was indistinguishable from a quiet
  queue, so the tick did nothing and slept the full interval — once per merge,
  on every delivery, which across a twelve-branch drain is roughly a fifth of
  the elapsed time spent waiting for an answer GitHub already had. The state is
  now named `settling` and the watcher comes back in five seconds instead of
  sixty. It is safe to return early precisely because a settling tick has, by
  construction, done nothing. One resolved entry is enough to stop settling and
  take a turn, so the queue cannot sit in a recompute loop.
