- Squash and rebase merging are now disabled on `monk-eee/MindLeak`, so the
  merge commit is the only button available. AGENTS.md has always asked for
  merge commits so that a commit id stays evidence, and ADR-0038 is explicit
  that squashing, rebasing and cherry-picking replace evidence-bearing commit
  identities — but nothing enforced it, so the rule depended on which button an
  agent clicked. Verified by reading the repository settings back rather than by
  intention: `allow_squash_merge` and `allow_rebase_merge` are both false and
  `allow_merge_commit` is true.
  This had already cost real time. PR #205 was armed with `--squash`, landing all
  245 lines under a new commit id; the merge audit compared ancestry, could not
  tell a squash from a branch that never merged, and reported the work as lost.
  Another agent confirmed that with `git merge-base --is-ancestor`, wrote it into
  durable knowledge as fact, and queued a pull request to restore code that was
  already on `main`.
  AGENTS.md now also states the test that distinguishes the two: `git cherry -v
  origin/main <branch>` compares patches, where `-` means an equivalent patch is
  already upstream and `+` means it never landed. Ancestry asks about commit
  identity and therefore answers "no" for work that is fully present. History
  still holds identities rewritten before the button was closed, so the
  distinction remains load-bearing even though new ones can no longer be
  created — `scripts/worktree-reclaim.mjs` depends on it to tell a merged
  worktree from an unmerged one.
  Checked before flipping: one pull request had auto-merge armed and it was
  armed with `MERGE`, so nothing in flight was broken by removing the other two
  methods.
