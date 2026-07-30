- A file saved in any worktree of this repository now reaches the graph under its
  canonical repository-relative id, instead of being refused because it came from
  a checkout the server was not rooted at. Measured 2026-07-30: of the 291
  `ingest_file` calls after the ingest guard landed, **203 were refused (69.8%)**,
  naming paths like
  `c:/Users/.../MindLeak-rustimports/scripts/silent-knowledge.mjs`. The graph was
  clean of duplicate identities partly because those files were not arriving at
  all — the guard had converted silent corruption into visible loss, and the loss
  was larger than the corruption.
  Every worktree of a repository shares one graph (ADR-0038), so a path under any
  of its worktrees is unambiguously the same file. The server now resolves those
  roots with `git worktree list` and treats every one of them as a candidate when
  placing a path; the longest match wins, and a root only matches on a path
  boundary, so `.../MindLeak` never swallows a path under `.../MindLeak-build`.
  Commit and execution sensors place their changed files the same way, so a
  commit touching a sibling checkout no longer drops those files either.
  Rooting each window at its own worktree (ADR-0073) remains the cheaper, more
  direct fix. This makes the answer the same whichever window did the saving,
  rather than leaving correctness to an operational habit. A path under no root
  of this repository is still refused, and when git cannot answer the behaviour
  degrades to the previous single-root placement rather than to a wrong id.
