- **Delivery has a queue again, and we run it ourselves.** ADR-0061 chose
  GitHub's merge queue; the `merge_queue` ruleset rule is refused on this
  repository, because merge queue requires an organisation-owned repo and this
  one belongs to a user account. The same endpoint accepts other rules with the
  same credentials, so it is the feature that is unavailable, not the request.
  `make queue` (ADR-0062) serialises the step that was actually contended:
  with eleven armed pull requests and up-to-dateness required, every merge makes
  the other ten stale, and each one that refreshes itself burns a full check run
  against a `main` the next merge invalidates — O(N²) runs that never drain. The
  queue brings exactly one branch up to date at a time, in the order they were
  armed, which makes it O(N). It **never merges**: merging stays with GitHub's
  auto-merge behind the same five required checks, so it cannot become a second
  route into `main` that branch protection does not govern. `make queue-watch`
  runs it as an agent.
