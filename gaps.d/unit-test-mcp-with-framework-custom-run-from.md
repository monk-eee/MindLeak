- **Unit Test MCP with `framework=custom` run from `editors/vscode` silently
  runs Cargo, not Vitest, and reports PASSED — CONFIRMED, config footgun.**
  Cargo walks up from `editors/vscode` and finds the workspace `Cargo.toml`, so
  the Rust suite runs and goes green while the extension tests never execute.
  Verified by breaking a `util.test.ts` assertion on purpose: `framework=custom`
  reported PASSED; `framework=vitest` with
  `root_dir=<repo>/editors/vscode` reported the real failure and the assertion
  diff. Any extension change validated through the custom adapter has a
  meaningless green behind it. Use `framework=vitest` for
  `editors/vscode`, and treat a suspiciously fast/slow duration as the tell.
