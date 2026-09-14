- **Direct Cargo builds can embed another repository's source identity.**
  `scripts/build-info.rs::git_output` selects the repository with `git -C` but
  inherits Git repository pointers. A native build of `02b8a284` under a
  deliberately foreign environment made `mindleak-mcp` identify itself as
  `0.1.7-alpha+f8b44c672e78`, the fixture revision. The original result is retained
  in `target/industrial-foreign-git-02b8a28424e00b55380b41ce0b1feb6665c9e121.result.json`.
  Industrial packaging now isolates the subprocess environment and verifies the
  Local MCP revision before publication. The shared Rust build helper remains
  exposed for other callers and needs its own isolation regression and fix;
  left for a separate scoped follow-up. No affected archive was installed or
  released.
