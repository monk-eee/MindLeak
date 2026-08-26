- **Unit Test MCP reports `PASSED` for `scripts/*.test.mjs`, which it never
  runs — OPEN in the tool itself; this repo's exposure to it is now removed.**
  The repository's guard tests are `node:test` files and no adapter covers
  them. Asked to run one with `framework=custom`, `run_tests` returned
  `status: PASSED` with `passed`/`failed`/`total` all zero. A red/green
  probe on 2026-07-29 proved the false green: an assertion that `1 === 2` inside
  `scripts/measure-tool-surface.test.mjs` still came back `PASSED`. — High
  impact: a suite that never executed is indistinguishable from a real green
  result. — Until an adapter exists, validate script tests with
  `make script-test` (`node scripts/script-tests.mjs`), which is what CI runs.

  **NARROWED 2026-08-26.** `AGENTS.md`'s own "Commands" section used to read
  "prefer the Unit Test MCP tools for test runs where available", actively
  directing every agent working here toward the tool this fragment (and its
  three siblings) measured giving false greens. That line is gone: the
  section now names the specific failure modes across all four fragments and
  tells agents to run `make`/`cargo`/`npm` directly instead, which is what CI
  trusts. This closes this repository's *dependency* on the broken tool, not
  the tool's own defect — a third-party MCP extension is not this repo's to
  fix, so the underlying bug stays OPEN and this fragment stays open with it.
