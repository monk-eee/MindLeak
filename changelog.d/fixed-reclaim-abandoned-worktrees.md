- `make reclaim` now takes back a worktree whose pull request was closed without
  merging, instead of keeping it forever. Such a branch never lands, so the rule
  that protects unlanded work was holding it permanently — which meant the more
  disciplined the fleet was about closing duplicate pull requests, the more dead
  worktrees it accumulated. A branch with unlanded commits and no pull request is
  still kept, because nobody has decided against that work yet, and the report
  now says whether a reclaim is "merged and idle" or "abandoned: its pull request
  closed unmerged".
