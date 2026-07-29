- **Unit Test MCP 1.3.6 cannot validate this workspace reliably.** — Its Vitest
  discovery finds `src/util.test.ts`, but `run_tests` reports a passing total of
  zero even for that explicit path. On Windows, a backslash Cargo root is
  rejected as `INVALID_ROOT_DIR`; normalizing it to forward slashes runs the
  custom command and surfaces failures, but successful runs still report zero
  tests. Vitest coverage also depends on drive-letter casing: a lowercase `c:`
  root duplicates every covered source as an uppercase `C:` zero-hit shadow,
  falsely reporting 38.64% lines; the canonical uppercase root produces the
  correct unique-file aggregate (89.19% lines / 84.85% branches). — High impact
  on local proof. — Left open in the external adapter; use a canonical uppercase
  Windows drive root for coverage, while CI's test counts remain authoritative.
