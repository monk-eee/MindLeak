- **Unit Test MCP reports `PASSED` for `scripts/*.test.mjs`, which it never
  runs — OPEN.** The repository's guard tests are `node:test` files and no
  adapter covers them. Asked to run one with `framework=custom`, `run_tests`
  returned `status: PASSED` with `passed`/`failed`/`total` all zero. A red/green
  probe on 2026-07-29 proved the false green: an assertion that `1 === 2` inside
  `scripts/measure-tool-surface.test.mjs` still came back `PASSED`. — High
  impact: a suite that never executed is indistinguishable from a real green
  result. — Until an adapter exists, validate script tests with
  `make script-test` (`node scripts/script-tests.mjs`), which is what CI runs.
