- **Unit Test MCP with `framework=custom` run from `editors/vscode` silently
  runs Cargo, not Vitest, and reports PASSED — CONFIRMED, config footgun in the
  tool itself; this repo no longer routes agents to it.**
  Cargo walks up from `editors/vscode` and finds the workspace `Cargo.toml`, so
  the Rust suite runs and goes green while the extension tests never execute.
  Verified by breaking a `util.test.ts` assertion on purpose: `framework=custom`
  reported PASSED; `framework=vitest` with
  `root_dir=<repo>/editors/vscode` reported the real failure and the assertion
  diff. Any extension change validated through the custom adapter has a
  meaningless green behind it. Use `framework=vitest` for
  `editors/vscode`, and treat a suspiciously fast/slow duration as the tell.

  **NARROWED 2026-08-26.** `AGENTS.md` used to tell every agent to prefer the
  Unit Test MCP tool for test runs; it now names this footgun (alongside its
  three siblings) directly and points at `npm --prefix editors/vscode test`
  instead. That closes the exposure this repository controls; the adapter's
  own silent-substitution bug is unchanged and still OPEN, since it lives in
  a third-party extension.
