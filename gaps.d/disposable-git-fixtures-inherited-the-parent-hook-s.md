- **Disposable Git fixtures inherited the parent hook's alternate index —
  FIXED.** — Committed-snapshot Cargo hooks set `GIT_INDEX_FILE`; child `git`
  commands in repository-state and publisher tests inherited it even when they
  changed CWD to a temporary repository. A fixture `git add README.md` therefore
  staged its one-line file into the parent index, and fixture-local `user.name` /
  `user.email` leaked into shared clone config. — High impact: the next scoped
  commit could carry a destructive parent-repository edit under the wrong
  identity. — **Fixed Jul 2026:** production repository discovery and every Rust
  / Node disposable Git harness clear `GIT_DIR`, `GIT_WORK_TREE`,
  `GIT_COMMON_DIR`, `GIT_INDEX_FILE`, and object-directory overrides before
  invoking Git. The contaminated README was restored exactly and local identity
  overrides were removed; focused and full pre-push suites pass.
