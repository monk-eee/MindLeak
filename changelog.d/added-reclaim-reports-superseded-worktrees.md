- `make reclaim` now reports worktrees that are kept only because they are dirty,
  where every uncommitted path already exists on `origin/main` and the branch's
  own commits have landed. Such a tree is otherwise kept forever — untracked
  files never become clean on their own, so the same refusal fires at every audit
  and the kept set only grows. The report never deletes anything and does not
  claim the work is obsolete: it states the facts and names the person as the one
  who decides. A single uncommitted path that exists nowhere upstream withdraws
  the whole report, because that tree contains new work.
