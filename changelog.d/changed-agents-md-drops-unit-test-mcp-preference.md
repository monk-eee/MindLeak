- **Changed:** `AGENTS.md` no longer tells agents to "prefer the Unit Test MCP
  tools for test runs where available." Four gap fragments
  (`gaps.d/unit-test-mcp-*.md`, `gaps.d/the-unit-test-mcp-cargo-adapter-hides-the.md`)
  independently measured that tool reporting `PASSED` for `scripts/*.test.mjs`
  suites it never executes, silently substituting Cargo for Vitest (and vice
  versa) depending on working directory, hiding a failing test's own name and
  assertion behind a bare compile-error-shaped message, and ignoring
  `test_pattern` while its shortened run time reads exactly like the filter
  worked. None of that is something this repository can fix — it is a
  third-party MCP extension — so the instruction now names the specific
  failure modes and points at `make`/`cargo`/`npm` directly, which is what CI
  already trusts as the verdict of record.
