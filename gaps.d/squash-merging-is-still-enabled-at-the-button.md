- **Squash and rebase merging are still enabled, and an audit can only notice
  afterwards — MEASURED, OPEN.** AGENTS.md asks for merge commits so that a
  commit id stays evidence, and ADR-0038 is explicit that cherry-picking,
  rebasing and squashing replace evidence-bearing commit identities. Nothing
  enforces it: the buttons are available on every pull request, and the rule
  therefore depends on which one an agent clicks.

  It has already cost real time. PR #205 was armed with `gh pr merge --squash`,
  which landed all 245 lines under a new commit id. `scripts/merge-audit.mjs`
  then compared ancestry, could not tell a squash from a branch that never
  merged, and reported the work as lost. Another agent confirmed that with
  `git merge-base --is-ancestor`, wrote it into durable knowledge as fact, and
  queued a follow-up pull request to restore code that was already on `main` —
  into the one file three agents were editing at the time. The audit now uses
  `git cherry`, which compares patches instead of ids, so the false alarm
  cannot recur; the destroyed commit identity is not recoverable, because
  making it an ancestor now would mean rewriting `main`.

  That fix is a report, not a prevention. The audit runs after the merge, and
  by then the only truthful thing it can say is that an identity AGENTS.md
  wanted has already gone. The prevention lives on the repository settings page:
  disabling squash merging and rebase merging on `monk-eee/MindLeak` leaves the
  merge commit as the only available button. Until that is done, the guidance
  is a convention that every agent can breach by accident, and the audit's
  rewritten-commit warning is the only thing standing behind it.
