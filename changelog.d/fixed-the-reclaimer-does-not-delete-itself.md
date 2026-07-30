- The reclaimer no longer deletes the worktree it is running in. Found on the
  first live run of `scripts/worktree-reclaim.mjs` against 94 real worktrees and
  shipped in the previous change without it: the tool's own worktree was merged,
  clean, idle, owned by this session and not mid-build, so every rule said
  reclaim it. Acting on that would have deleted `target/` out from under the
  running process and then called `git worktree remove` on the checkout it was
  executing in.
  It did not happen only because the report prints before anything is deleted
  and was read before `--reclaim` was passed. That is the reporting default
  doing its job rather than a guard, and it protects nobody who trusts the tool
  enough to pass `--reclaim` immediately — which is what AGENTS.md now tells
  every agent to do once their PR merges. The instruction and the defect shipped
  together.
  The refusal names its reason like every other rule, because a worktree that
  simply vanished from the report would read as one the tool failed to see.
  Compared on resolved paths, and decided before the cheaper exits, since the
  tool can equally be run from the bare primary or a detached worktree.
