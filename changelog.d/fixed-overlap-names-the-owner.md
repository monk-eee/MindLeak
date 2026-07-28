- **The overlap warning no longer blames a peer for your own claim.**
  `canonical-push` reported "another agent has a live claim over paths this
  branch touches" and named a task that was the publishing agent's own.
  Ownership was decided only by whether the task appeared in a list the caller
  derived from its own live claims — and when identity resolution drifted during
  the ADR-0054 migration, that list came back empty while the claim was plainly
  its own. The notice now compares the claim's owner against this session
  directly, so it is right even when the derived list is wrong. When the session
  has no identity to compare against it says so, rather than asserting the claim
  belongs to someone else. A warning that names the wrong party is worse than no
  warning: it sends someone to ask a question of themselves, and it trains
  readers to discount the next one.
